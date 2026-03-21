mod endpoint;
mod error;
mod metrics;
mod relay;
mod spec;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::time::Instant;

use crate::error::SocoreError;
use crate::spec::{EndpointOptions, EndpointPlan, EndpointSpec, ProxyType};

#[derive(Debug, Parser)]
#[command(name = "socat")]
#[command(about = "Modern socat rewrite: compatibility path + simple path")]
struct Cli {
    #[arg(global = true, long, help = "Print machine-readable errors/info")]
    json: bool,
    #[arg(global = true, long, help = "Only parse and show resolved plan")]
    dry_run: bool,
    #[arg(
        global = true,
        long,
        value_enum,
        help = "Apply built-in connection profile defaults"
    )]
    profile: Option<ProfilePreset>,
    #[arg(
        global = true,
        long,
        help = "Expose Prometheus metrics on host:port (e.g. 0.0.0.0:9464)"
    )]
    metrics_bind: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(value_name = "ADDRESS", num_args = 0..=2)]
    legacy: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Link(LinkArgs),
    Tunnel(TunnelArgs),
    Plan(LinkArgs),
    Validate(LinkArgs),
    Explain { address: String },
    Inventory,
}

#[derive(Debug, Args)]
struct LinkArgs {
    #[arg(long)]
    from: String,
    #[arg(long)]
    to: String,
}

#[derive(Debug, Args)]
struct TunnelArgs {
    #[arg(long, default_value = "stdio://")]
    from: String,
    #[arg(long, required = true, num_args = 1.., action = clap::ArgAction::Append)]
    via: Vec<String>,
    #[arg(long, help = "final tunnel target in host:port form")]
    to: String,
}

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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProfilePreset {
    Dev,
    Prod,
    Lan,
    Wan,
}

impl ProfilePreset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Prod => "prod",
            Self::Lan => "lan",
            Self::Wan => "wan",
        }
    }

    fn defaults(self) -> EndpointOptions {
        match self {
            Self::Dev => EndpointOptions {
                connect_timeout_ms: Some(3_000),
                retry: Some(1),
                retry_delay_ms: Some(200),
                retry_backoff: Some(crate::spec::RetryBackoff::Constant),
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
            },
            Self::Prod => EndpointOptions {
                connect_timeout_ms: Some(5_000),
                retry: Some(5),
                retry_delay_ms: Some(500),
                retry_backoff: Some(crate::spec::RetryBackoff::Exponential),
                retry_max_delay_ms: Some(10_000),
                tls_verify: Some(true),
                tls_sni: None,
            },
            Self::Lan => EndpointOptions {
                connect_timeout_ms: Some(1_500),
                retry: Some(2),
                retry_delay_ms: Some(100),
                retry_backoff: Some(crate::spec::RetryBackoff::Constant),
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
            },
            Self::Wan => EndpointOptions {
                connect_timeout_ms: Some(10_000),
                retry: Some(4),
                retry_delay_ms: Some(1_000),
                retry_backoff: Some(crate::spec::RetryBackoff::Exponential),
                retry_max_delay_ms: Some(15_000),
                tls_verify: Some(true),
                tls_sni: None,
            },
        }
    }
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
        (Some(Command::Explain { address }), _) => {
            let plan = apply_profile(parse_endpoint_plan(&address)?, profile);
            emit(cli.json, &plan)?;
            Ok(())
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
            run_link_and_maybe_emit_report("simple", profile, left, right, cli.json).await
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
            run_link_and_maybe_emit_report("tunnel", profile, left, right, cli.json).await
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
            run_link_and_maybe_emit_report("legacy", profile, left, right, cli.json).await
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
            }
            Ok(())
        }
        Err(err) => {
            metrics::record_connection_failed();
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::spec::{EndpointOptions, EndpointSpec};

    use super::{
        EndpointPlan, ProfilePreset, apply_profile, build_tunnel_endpoint,
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
}
