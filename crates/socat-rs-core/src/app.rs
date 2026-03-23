use anyhow::Context;
use clap::Parser;
use serde::Serialize;
use std::time::Instant;

use crate::cli::{Cli, Command, ProfilePreset};
use crate::error::SocoreError;
use crate::spec::{EndpointOptions, EndpointPlan, EndpointSpec, ProxyType};
use crate::{endpoint, metrics, relay, spec};

#[derive(Debug, Serialize)]
struct PlanOutput {
    mode: &'static str,
    profile: Option<String>,
    valid: bool,
    from: EndpointPlan,
    to: EndpointPlan,
}

#[derive(Debug, Serialize)]
struct RunReport {
    mode: &'static str,
    profile: Option<String>,
    from: EndpointPlan,
    to: EndpointPlan,
    stats: relay::RelayStats,
    duration_ms: u128,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    profile: Option<String>,
    plan: EndpointPlan,
    ok: bool,
    latency_ms: u128,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    supports_unix_socket: bool,
    supports_named_pipe: bool,
    tls_listen_identity_set: bool,
    recommended_release_targets: Vec<&'static str>,
}

pub fn run() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;

    let result = runtime.block_on(async move {
        if let Some(bind) = cli.metrics_bind.clone() {
            tokio::spawn(async move {
                if let Err(err) = metrics::serve_prometheus(bind).await {
                    tracing::warn!("metrics server exited: {err}");
                }
            });
        }
        dispatch(cli).await
    });

    if let Err(err) = result {
        return Err(anyhow::Error::new(err));
    }
    Ok(())
}

async fn dispatch(cli: Cli) -> Result<(), SocoreError> {
    let profile = cli.profile;
    match (cli.command, cli.legacy.as_slice()) {
        (Some(Command::Inventory), _) => {
            print_inventory(cli.json);
            Ok(())
        }
        (Some(Command::Doctor), _) => {
            let out = DoctorReport {
                version: env!("CARGO_PKG_VERSION"),
                os: std::env::consts::OS,
                arch: std::env::consts::ARCH,
                supports_unix_socket: cfg!(unix),
                supports_named_pipe: cfg!(windows),
                tls_listen_identity_set: std::env::var("SOCAT_RS_TLS_PKCS12").is_ok(),
                recommended_release_targets: default_release_targets(),
            };
            emit(cli.json, &out)
        }
        (Some(Command::Explain { address }), _) => {
            let plan = apply_profile(parse_endpoint_plan(&address)?, profile);
            emit(cli.json, &plan)?;
            Ok(())
        }
        (Some(Command::Check { address }), _) => {
            let plan = apply_profile(parse_endpoint_plan(&address)?, profile);
            run_check(plan, profile, cli.json).await
        }
        (Some(Command::Plan(args)), _) | (Some(Command::Validate(args)), _) => {
            let left = apply_profile(parse_endpoint_plan(&args.from)?, profile);
            let right = apply_profile(parse_endpoint_plan(&args.to)?, profile);
            let out = PlanOutput {
                mode: "plan",
                profile: profile.map(|p| p.as_str().to_string()),
                valid: true,
                from: left,
                to: right,
            };
            emit(cli.json, &out)?;
            Ok(())
        }
        (Some(Command::Link(args)), _) => {
            let left = apply_profile(parse_endpoint_plan(&args.from)?, profile);
            let right = apply_profile(parse_endpoint_plan(&args.to)?, profile);
            if cli.dry_run {
                let out = PlanOutput {
                    mode: "simple",
                    profile: profile.map(|p| p.as_str().to_string()),
                    valid: true,
                    from: left,
                    to: right,
                };
                emit(cli.json, &out)?;
                return Ok(());
            }
            run_link_and_maybe_emit_report(
                "simple",
                profile,
                left,
                right,
                cli.json,
                cli.report_file.as_deref(),
            )
            .await
        }
        (Some(Command::Tunnel(args)), _) => {
            let left = apply_profile(parse_endpoint_plan(&args.from)?, profile);
            let right_endpoint = build_tunnel_endpoint(&args.via, &args.to)?;
            let right = apply_profile(
                EndpointPlan {
                    endpoint: right_endpoint,
                    options: EndpointOptions::default(),
                },
                profile,
            );
            if cli.dry_run {
                let out = PlanOutput {
                    mode: "tunnel",
                    profile: profile.map(|p| p.as_str().to_string()),
                    valid: true,
                    from: left,
                    to: right,
                };
                emit(cli.json, &out)?;
                return Ok(());
            }
            run_link_and_maybe_emit_report(
                "tunnel",
                profile,
                left,
                right,
                cli.json,
                cli.report_file.as_deref(),
            )
            .await
        }
        (None, [left, right]) => {
            let left = apply_profile(spec::parse_legacy_with_options(left)?, profile);
            let right = apply_profile(spec::parse_legacy_with_options(right)?, profile);
            if cli.dry_run {
                let out = PlanOutput {
                    mode: "legacy",
                    profile: profile.map(|p| p.as_str().to_string()),
                    valid: true,
                    from: left,
                    to: right,
                };
                emit(cli.json, &out)?;
                return Ok(());
            }
            run_link_and_maybe_emit_report(
                "legacy",
                profile,
                left,
                right,
                cli.json,
                cli.report_file.as_deref(),
            )
            .await
        }
        _ => Err(SocoreError::InvalidAddress(
            "expected either: `socat <addr1> <addr2>` or `socat link --from ... --to ...`"
                .to_string(),
        )),
    }
}

fn print_inventory(json: bool) {
    #[derive(Debug, Serialize)]
    struct Inventory<'a> {
        legacy_address_keywords: usize,
        legacy_option_keywords: usize,
        legacy_address_handlers: usize,
        implemented_modes: &'a [&'a str],
        built_in_profiles: &'a [&'a str],
    }

    let out = Inventory {
        legacy_address_keywords: socat_rs_compat::LEGACY_ADDRESS_KEYWORDS,
        legacy_option_keywords: socat_rs_compat::LEGACY_OPTION_KEYWORDS,
        legacy_address_handlers: socat_rs_compat::LEGACY_ADDRESS_HANDLERS,
        implemented_modes: socat_rs_compat::COMPAT_MODES,
        built_in_profiles: &["dev", "prod", "lan", "wan"],
    };

    let _ = emit(json, &out);
}

fn emit<T: Serialize + std::fmt::Debug>(json: bool, value: &T) -> Result<(), SocoreError> {
    if json {
        let txt = serde_json::to_string_pretty(value)
            .map_err(|e| SocoreError::InvalidAddress(e.to_string()))?;
        println!("{txt}");
    } else {
        println!("{value:#?}");
    }
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn parse_endpoint_plan(input: &str) -> Result<EndpointPlan, SocoreError> {
    if input.contains("://") {
        spec::parse_simple_uri_with_options(input)
    } else {
        spec::parse_legacy_with_options(input)
    }
}

fn apply_profile(mut plan: EndpointPlan, profile: Option<ProfilePreset>) -> EndpointPlan {
    if let Some(preset) = profile {
        let defaults = preset.defaults();
        plan.options.connect_timeout_ms = plan
            .options
            .connect_timeout_ms
            .or(defaults.connect_timeout_ms);
        plan.options.retry = plan.options.retry.or(defaults.retry);
        plan.options.retry_delay_ms = plan.options.retry_delay_ms.or(defaults.retry_delay_ms);
        plan.options.retry_backoff = plan.options.retry_backoff.or(defaults.retry_backoff);
        plan.options.retry_max_delay_ms = plan
            .options
            .retry_max_delay_ms
            .or(defaults.retry_max_delay_ms);
        plan.options.tls_verify = plan.options.tls_verify.or(defaults.tls_verify);
        plan.options.tls_sni = plan.options.tls_sni.or(defaults.tls_sni);
        plan.options.tls_ca_file = plan.options.tls_ca_file.or(defaults.tls_ca_file);
        plan.options.tls_client_pkcs12 = plan
            .options
            .tls_client_pkcs12
            .or(defaults.tls_client_pkcs12);
        plan.options.tls_client_password = plan
            .options
            .tls_client_password
            .or(defaults.tls_client_password);
    }
    plan
}

fn ensure_tunnel_via_has_target(via: &str, to: &str) -> Result<String, SocoreError> {
    let mut url = url::Url::parse(via).map_err(|_| SocoreError::InvalidAddress(via.to_string()))?;
    let is_proxy = matches!(
        url.scheme(),
        "socks4" | "socks4a" | "socks5" | "http-proxy" | "proxy"
    );
    if !is_proxy {
        return Err(SocoreError::InvalidAddress(
            "tunnel --via currently supports socks4/socks4a/socks5/http-proxy/proxy schemes"
                .to_string(),
        ));
    }

    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    pairs.retain(|(k, _)| k != "target");
    pairs.push(("target".to_string(), to.to_string()));
    url.set_query(None);
    {
        let mut q = url.query_pairs_mut();
        for (k, v) in pairs {
            q.append_pair(&k, &v);
        }
    }
    Ok(url.into())
}

fn build_tunnel_endpoint(vias: &[String], target: &str) -> Result<EndpointSpec, SocoreError> {
    let mut hops: Vec<String> = Vec::new();
    for via in vias {
        for piece in via.split(',') {
            let trimmed = piece.trim();
            if !trimmed.is_empty() {
                hops.push(trimmed.to_string());
            }
        }
    }
    if hops.is_empty() {
        return Err(SocoreError::InvalidAddress(
            "tunnel requires at least one --via hop".to_string(),
        ));
    }

    if hops.len() == 1 {
        let via = ensure_tunnel_via_has_target(&hops[0], target)?;
        return Ok(parse_endpoint_plan(&via)?.endpoint);
    }

    let mut parsed = Vec::with_capacity(hops.len());
    for via in &hops {
        parsed.push(spec::parse_proxy_hop_uri(via)?);
    }
    if matches!(parsed.last().map(|h| h.kind), Some(ProxyType::Socks4)) {
        let (host, _) = target.rsplit_once(':').ok_or_else(|| {
            SocoreError::InvalidAddress(format!("invalid tunnel target host:port: {target}"))
        })?;
        if host.parse::<std::net::Ipv4Addr>().is_err() {
            return Err(SocoreError::InvalidAddress(
                "last SOCKS4 hop requires IPv4 target; use socks4a or socks5 for domain targets"
                    .to_string(),
            ));
        }
    }
    Ok(EndpointSpec::ProxyChain {
        hops: parsed,
        target: target.to_string(),
    })
}

async fn run_link_and_maybe_emit_report(
    mode: &'static str,
    profile: Option<ProfilePreset>,
    left: EndpointPlan,
    right: EndpointPlan,
    json: bool,
    report_file: Option<&str>,
) -> Result<(), SocoreError> {
    metrics::record_connection_start();
    let started = Instant::now();
    match relay::bridge_with_plans(left.clone(), right.clone()).await {
        Ok(stats) => {
            metrics::record_bytes(stats.bytes_left_to_right, stats.bytes_right_to_left);
            if json {
                let report = RunReport {
                    mode,
                    profile: profile.map(|p| p.as_str().to_string()),
                    from: left,
                    to: right,
                    stats,
                    duration_ms: started.elapsed().as_millis(),
                };
                emit(true, &report)?;
                if let Some(path) = report_file {
                    write_json_file(path, &report)?;
                }
            }
            Ok(())
        }
        Err(err) => {
            metrics::record_connection_failed();
            Err(err)
        }
    }
}

async fn run_check(
    plan: EndpointPlan,
    profile: Option<ProfilePreset>,
    json: bool,
) -> Result<(), SocoreError> {
    let started = Instant::now();
    let result = endpoint::open_with_options(plan.endpoint.clone(), &plan.options).await;
    let report = match result {
        Ok(_) => CheckReport {
            profile: profile.map(|p| p.as_str().to_string()),
            plan,
            ok: true,
            latency_ms: started.elapsed().as_millis(),
            error: None,
        },
        Err(err) => CheckReport {
            profile: profile.map(|p| p.as_str().to_string()),
            plan,
            ok: false,
            latency_ms: started.elapsed().as_millis(),
            error: Some(err.to_string()),
        },
    };
    emit(json, &report)?;
    Ok(())
}

fn write_json_file<T: Serialize>(path: &str, value: &T) -> Result<(), SocoreError> {
    let txt = serde_json::to_string_pretty(value)
        .map_err(|e| SocoreError::InvalidAddress(format!("failed to encode report json: {e}")))?;
    std::fs::write(path, txt)
        .map_err(|e| SocoreError::InvalidAddress(format!("failed to write report file: {e}")))?;
    Ok(())
}

fn default_release_targets() -> Vec<&'static str> {
    vec![
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ]
}

#[cfg(test)]
mod tests {
    use crate::spec::{EndpointOptions, EndpointSpec};

    use super::{
        EndpointPlan, ProfilePreset, apply_profile, build_tunnel_endpoint, default_release_targets,
        ensure_tunnel_via_has_target, parse_endpoint_plan,
    };

    #[test]
    fn profile_fills_missing_options() {
        let plan = EndpointPlan {
            endpoint: EndpointSpec::TcpConnect("127.0.0.1:80".to_string()),
            options: EndpointOptions {
                connect_timeout_ms: None,
                retry: Some(9),
                retry_delay_ms: None,
                retry_backoff: None,
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
                tls_ca_file: None,
                tls_client_pkcs12: None,
                tls_client_password: None,
            },
        };
        let got = apply_profile(plan, Some(ProfilePreset::Lan));
        assert_eq!(got.options.connect_timeout_ms, Some(1_500));
        assert_eq!(got.options.retry, Some(9));
        assert_eq!(got.options.retry_delay_ms, Some(100));
    }

    #[test]
    fn parse_endpoint_plan_auto_detects_legacy_and_simple() {
        let legacy = parse_endpoint_plan("TCP:127.0.0.1:8080").expect("legacy");
        assert!(matches!(legacy.endpoint, EndpointSpec::TcpConnect(_)));

        let simple = parse_endpoint_plan("tcp://127.0.0.1:8080").expect("simple");
        assert!(matches!(simple.endpoint, EndpointSpec::TcpConnect(_)));
    }

    #[test]
    fn tunnel_via_injects_target() {
        let via = ensure_tunnel_via_has_target("socks5://u:p@127.0.0.1:1080", "example.com:443")
            .expect("inject target");
        assert!(via.contains("target=example.com%3A443"));
    }

    #[test]
    fn tunnel_multi_hop_builds_proxy_chain() {
        let endpoint = build_tunnel_endpoint(
            &[
                "socks5://127.0.0.1:1080".to_string(),
                "http-proxy://127.0.0.1:8080".to_string(),
            ],
            "example.com:443",
        )
        .expect("build endpoint");
        assert!(
            matches!(endpoint, EndpointSpec::ProxyChain { hops, target } if hops.len() == 2 && target == "example.com:443")
        );
    }

    #[test]
    fn write_json_file_works() {
        let mut out = std::env::temp_dir();
        out.push(format!("socat-rs-report-{}.json", std::process::id()));
        let payload = serde_json::json!({ "ok": true });
        super::write_json_file(out.to_str().expect("path str"), &payload).expect("write file");
        let text = std::fs::read_to_string(&out).expect("read");
        assert!(text.contains("\"ok\": true"));
        let _ = std::fs::remove_file(out);
    }

    #[test]
    fn doctor_targets_include_major_platforms() {
        let targets = default_release_targets();
        assert!(targets.contains(&"x86_64-unknown-linux-gnu"));
        assert!(targets.contains(&"aarch64-unknown-linux-gnu"));
        assert!(targets.contains(&"x86_64-pc-windows-msvc"));
        assert!(targets.contains(&"aarch64-pc-windows-msvc"));
        assert!(targets.contains(&"x86_64-apple-darwin"));
        assert!(targets.contains(&"aarch64-apple-darwin"));
    }
}
