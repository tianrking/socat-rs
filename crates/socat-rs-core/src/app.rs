use anyhow::Context;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::cli::{Cli, Command, ProfilePreset, RunArgs};
use crate::error::SocoreError;
use crate::spec::{EndpointOptions, EndpointPlan, EndpointSpec, ProxyType};
use crate::{endpoint, metrics, relay, spec};

const JSON_SCHEMA_VERSION: &str = "1.0.0";

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

#[derive(Debug, Serialize)]
struct Inventory<'a> {
    legacy_address_keywords: usize,
    legacy_option_keywords: usize,
    legacy_address_handlers: usize,
    implemented_modes: &'a [&'a str],
    built_in_profiles: &'a [&'a str],
}

#[derive(Debug, Serialize)]
struct JsonError {
    code: &'static str,
    message: String,
    hint: String,
}

#[derive(Debug, Deserialize)]
struct JsonRunInput {
    mode: String,
    from: Option<String>,
    to: Option<String>,
    via: Option<Vec<String>>,
    address: Option<String>,
    legacy: Option<Vec<String>>,
    profile: Option<ProfilePreset>,
    dry_run: Option<bool>,
    json: Option<bool>,
    report_file: Option<String>,
}

pub fn run() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let cli_for_error = cli.clone();
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

    match result {
        Ok(()) => Ok(()),
        Err(err) if cli_for_error.json => {
            let command = command_name(cli_for_error.command.as_ref());
            let input = build_cli_input_payload(&cli_for_error);
            emit_json_envelope(
                false,
                command,
                input,
                Value::Null,
                Value::Null,
                Some(json_error_from_socore_error(&err)),
                vec![error_hint_action(&err)],
            )?;
            Ok(())
        }
        Err(err) => Err(anyhow::Error::new(err)),
    }
}

async fn dispatch(cli: Cli) -> Result<(), SocoreError> {
    let profile = cli.profile;
    match (cli.command, cli.legacy.as_slice()) {
        (Some(Command::Inventory), _) => {
            let out = inventory_output();
            if cli.json {
                emit_json_envelope(
                    true,
                    "inventory",
                    json!({}),
                    Value::Null,
                    to_json_value(&out)?,
                    None,
                    vec!["Use `socat explain <address>` to inspect one endpoint".to_string()],
                )
            } else {
                emit(false, &out)
            }
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
            if cli.json {
                emit_json_envelope(
                    true,
                    "doctor",
                    json!({}),
                    json!({
                        "normalized_endpoints": Value::Null,
                        "executable_command": "socat doctor",
                    }),
                    to_json_value(&out)?,
                    None,
                    vec![
                        "Use `socat --json plan --from ... --to ...` before execution".to_string(),
                    ],
                )
            } else {
                emit(false, &out)
            }
        }
        (Some(Command::Explain { address }), _) => {
            let plan = apply_profile(parse_endpoint_plan(&address)?, profile);
            if cli.json {
                let normalized = endpoint_plan_to_uri(&plan);
                emit_json_envelope(
                    true,
                    "explain",
                    json!({ "address": address }),
                    json!({
                        "normalized_endpoints": {"address": normalized},
                        "executable_command": format!("socat link --from {} --to stdio://", normalized),
                    }),
                    to_json_value(&plan)?,
                    None,
                    vec!["Use the suggested command in `plan.executable_command`".to_string()],
                )
            } else {
                emit(false, &plan)
            }
        }
        (Some(Command::Check { address }), _) => {
            let plan = apply_profile(parse_endpoint_plan(&address)?, profile);
            let report = run_check(plan.clone(), profile).await;
            if cli.json {
                emit_json_envelope(
                    report.ok,
                    "check",
                    json!({ "address": address, "profile": profile.map(|p| p.as_str()) }),
                    json!({
                        "normalized_endpoints": {"address": endpoint_plan_to_uri(&plan)},
                        "executable_command": format!("socat check {}", endpoint_plan_to_uri(&plan)),
                    }),
                    to_json_value(&report)?,
                    if report.ok {
                        None
                    } else {
                        Some(JsonError {
                            code: "E_CHECK_FAILED",
                            message: report
                                .error
                                .clone()
                                .unwrap_or_else(|| "check failed".to_string()),
                            hint: "Review endpoint parameters or network reachability".to_string(),
                        })
                    },
                    if report.ok {
                        vec!["Endpoint is reachable; you can run `socat link`".to_string()]
                    } else {
                        vec!["Run `socat explain <address>` for normalized details".to_string()]
                    },
                )
            } else {
                emit(false, &report)
            }
        }
        (Some(Command::Plan(args)), _) | (Some(Command::Validate(args)), _) => {
            let left = apply_profile(parse_endpoint_plan(&args.from)?, profile);
            let right = apply_profile(parse_endpoint_plan(&args.to)?, profile);
            let out = PlanOutput {
                mode: "plan",
                profile: profile.map(|p| p.as_str().to_string()),
                valid: true,
                from: left.clone(),
                to: right.clone(),
            };
            if cli.json {
                emit_json_envelope(
                    true,
                    "plan",
                    json!({ "from": args.from, "to": args.to, "profile": profile.map(|p| p.as_str()) }),
                    plan_payload("plan", &left, &right),
                    to_json_value(&out)?,
                    None,
                    vec!["Run the generated `plan.executable_command` to execute".to_string()],
                )
            } else {
                emit(false, &out)
            }
        }
        (Some(Command::Link(args)), _) => {
            let left = apply_profile(parse_endpoint_plan(&args.from)?, profile);
            let right = apply_profile(parse_endpoint_plan(&args.to)?, profile);
            let plan = plan_payload("simple", &left, &right);

            if cli.dry_run {
                if cli.json {
                    emit_json_envelope(
                        true,
                        "link",
                        json!({
                            "from": args.from,
                            "to": args.to,
                            "profile": profile.map(|p| p.as_str()),
                            "dry_run": true,
                        }),
                        plan,
                        json!({ "valid": true, "mode": "simple" }),
                        None,
                        vec!["Dry-run succeeded; remove `--dry-run` to execute".to_string()],
                    )
                } else {
                    emit(
                        false,
                        &PlanOutput {
                            mode: "simple",
                            profile: profile.map(|p| p.as_str().to_string()),
                            valid: true,
                            from: left,
                            to: right,
                        },
                    )
                }
            } else {
                let report = run_link_and_maybe_emit_report(
                    "simple",
                    profile,
                    left,
                    right,
                    cli.report_file.as_deref(),
                )
                .await?;
                if cli.json {
                    emit_json_envelope(
                        true,
                        "link",
                        json!({
                            "from": args.from,
                            "to": args.to,
                            "profile": profile.map(|p| p.as_str()),
                            "dry_run": false,
                        }),
                        plan,
                        to_json_value(&report)?,
                        None,
                        vec!["Link completed".to_string()],
                    )
                } else {
                    Ok(())
                }
            }
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
            let plan = plan_payload("tunnel", &left, &right);

            if cli.dry_run {
                if cli.json {
                    emit_json_envelope(
                        true,
                        "tunnel",
                        json!({
                            "from": args.from,
                            "via": args.via,
                            "to": args.to,
                            "profile": profile.map(|p| p.as_str()),
                            "dry_run": true,
                        }),
                        plan,
                        json!({ "valid": true, "mode": "tunnel" }),
                        None,
                        vec!["Dry-run succeeded; remove `--dry-run` to execute".to_string()],
                    )
                } else {
                    emit(
                        false,
                        &PlanOutput {
                            mode: "tunnel",
                            profile: profile.map(|p| p.as_str().to_string()),
                            valid: true,
                            from: left,
                            to: right,
                        },
                    )
                }
            } else {
                let report = run_link_and_maybe_emit_report(
                    "tunnel",
                    profile,
                    left,
                    right,
                    cli.report_file.as_deref(),
                )
                .await?;
                if cli.json {
                    emit_json_envelope(
                        true,
                        "tunnel",
                        json!({
                            "from": args.from,
                            "via": args.via,
                            "to": args.to,
                            "profile": profile.map(|p| p.as_str()),
                            "dry_run": false,
                        }),
                        plan,
                        to_json_value(&report)?,
                        None,
                        vec!["Tunnel completed".to_string()],
                    )
                } else {
                    Ok(())
                }
            }
        }
        (Some(Command::Run(args)), _) => run_from_json_input(args, profile, cli.json).await,
        (None, [left, right]) => {
            let left_plan = apply_profile(spec::parse_legacy_with_options(left)?, profile);
            let right_plan = apply_profile(spec::parse_legacy_with_options(right)?, profile);
            let plan = plan_payload("legacy", &left_plan, &right_plan);

            if cli.dry_run {
                if cli.json {
                    emit_json_envelope(
                        true,
                        "legacy",
                        json!({
                            "legacy": [left, right],
                            "profile": profile.map(|p| p.as_str()),
                            "dry_run": true,
                        }),
                        plan,
                        json!({ "valid": true, "mode": "legacy" }),
                        None,
                        vec!["Dry-run succeeded; remove `--dry-run` to execute".to_string()],
                    )
                } else {
                    emit(
                        false,
                        &PlanOutput {
                            mode: "legacy",
                            profile: profile.map(|p| p.as_str().to_string()),
                            valid: true,
                            from: left_plan,
                            to: right_plan,
                        },
                    )
                }
            } else {
                let report = run_link_and_maybe_emit_report(
                    "legacy",
                    profile,
                    left_plan,
                    right_plan,
                    cli.report_file.as_deref(),
                )
                .await?;
                if cli.json {
                    emit_json_envelope(
                        true,
                        "legacy",
                        json!({
                            "legacy": [left, right],
                            "profile": profile.map(|p| p.as_str()),
                            "dry_run": false,
                        }),
                        plan,
                        to_json_value(&report)?,
                        None,
                        vec!["Legacy link completed".to_string()],
                    )
                } else {
                    Ok(())
                }
            }
        }
        _ => Err(SocoreError::InvalidAddress(
            "expected either: `socat <addr1> <addr2>` or `socat link --from ... --to ...`"
                .to_string(),
        )),
    }
}

async fn run_from_json_input(
    args: RunArgs,
    global_profile: Option<ProfilePreset>,
    global_json: bool,
) -> Result<(), SocoreError> {
    let raw = read_json_input(&args.input_json)?;
    let req: JsonRunInput = serde_json::from_str(&raw).map_err(|e| {
        SocoreError::InvalidAddress(format!("invalid run input json: {e}; raw={raw}"))
    })?;

    let mode = req.mode.to_ascii_lowercase();
    let effective_json = global_json || req.json.unwrap_or(true);
    let profile = req.profile.or(global_profile);
    let dry_run = req.dry_run.unwrap_or(false);

    let synthesized = match mode.as_str() {
        "link" => Cli {
            json: effective_json,
            dry_run,
            profile,
            metrics_bind: None,
            report_file: req.report_file,
            command: Some(Command::Link(crate::cli::LinkArgs {
                from: require_field(req.from, "from")?,
                to: require_field(req.to, "to")?,
            })),
            legacy: Vec::new(),
        },
        "tunnel" => Cli {
            json: effective_json,
            dry_run,
            profile,
            metrics_bind: None,
            report_file: req.report_file,
            command: Some(Command::Tunnel(crate::cli::TunnelArgs {
                from: req.from.unwrap_or_else(|| "stdio://".to_string()),
                via: req
                    .via
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| SocoreError::InvalidAddress("missing field: via".to_string()))?,
                to: require_field(req.to, "to")?,
            })),
            legacy: Vec::new(),
        },
        "plan" => Cli {
            json: effective_json,
            dry_run: false,
            profile,
            metrics_bind: None,
            report_file: None,
            command: Some(Command::Plan(crate::cli::LinkArgs {
                from: require_field(req.from, "from")?,
                to: require_field(req.to, "to")?,
            })),
            legacy: Vec::new(),
        },
        "validate" => Cli {
            json: effective_json,
            dry_run: false,
            profile,
            metrics_bind: None,
            report_file: None,
            command: Some(Command::Validate(crate::cli::LinkArgs {
                from: require_field(req.from, "from")?,
                to: require_field(req.to, "to")?,
            })),
            legacy: Vec::new(),
        },
        "check" => Cli {
            json: effective_json,
            dry_run: false,
            profile,
            metrics_bind: None,
            report_file: None,
            command: Some(Command::Check {
                address: require_field(req.address, "address")?,
            }),
            legacy: Vec::new(),
        },
        "explain" => Cli {
            json: effective_json,
            dry_run: false,
            profile,
            metrics_bind: None,
            report_file: None,
            command: Some(Command::Explain {
                address: require_field(req.address, "address")?,
            }),
            legacy: Vec::new(),
        },
        "inventory" => Cli {
            json: effective_json,
            dry_run: false,
            profile,
            metrics_bind: None,
            report_file: None,
            command: Some(Command::Inventory),
            legacy: Vec::new(),
        },
        "doctor" => Cli {
            json: effective_json,
            dry_run: false,
            profile,
            metrics_bind: None,
            report_file: None,
            command: Some(Command::Doctor),
            legacy: Vec::new(),
        },
        "legacy" => {
            let legacy = if let Some(pair) = req.legacy {
                pair
            } else {
                vec![
                    require_field(req.from, "from")?,
                    require_field(req.to, "to")?,
                ]
            };
            if legacy.len() != 2 {
                return Err(SocoreError::InvalidAddress(
                    "legacy mode requires exactly 2 addresses".to_string(),
                ));
            }
            Cli {
                json: effective_json,
                dry_run,
                profile,
                metrics_bind: None,
                report_file: req.report_file,
                command: None,
                legacy,
            }
        }
        _ => {
            return Err(SocoreError::InvalidAddress(format!(
                "unsupported run mode: {}",
                req.mode
            )));
        }
    };

    Box::pin(dispatch(synthesized)).await
}

fn read_json_input(path: &str) -> Result<String, SocoreError> {
    if path == "-" {
        let mut raw = String::new();
        std::io::stdin()
            .read_to_string(&mut raw)
            .map_err(SocoreError::Io)?;
        if raw.trim().is_empty() {
            return Err(SocoreError::InvalidAddress(
                "stdin JSON input is empty".to_string(),
            ));
        }
        return Ok(raw);
    }
    std::fs::read_to_string(path).map_err(SocoreError::Io)
}

fn require_field(value: Option<String>, key: &str) -> Result<String, SocoreError> {
    value.ok_or_else(|| SocoreError::InvalidAddress(format!("missing field: {key}")))
}

fn inventory_output() -> Inventory<'static> {
    Inventory {
        legacy_address_keywords: socat_rs_compat::LEGACY_ADDRESS_KEYWORDS,
        legacy_option_keywords: socat_rs_compat::LEGACY_OPTION_KEYWORDS,
        legacy_address_handlers: socat_rs_compat::LEGACY_ADDRESS_HANDLERS,
        implemented_modes: socat_rs_compat::COMPAT_MODES,
        built_in_profiles: &["dev", "prod", "lan", "wan"],
    }
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

fn emit_json_envelope(
    ok: bool,
    command: &str,
    input: Value,
    plan: Value,
    result: Value,
    error: Option<JsonError>,
    next_actions: Vec<String>,
) -> Result<(), SocoreError> {
    let payload = json!({
        "schema_version": JSON_SCHEMA_VERSION,
        "ok": ok,
        "command": command,
        "input": input,
        "plan": plan,
        "result": result,
        "error": error,
        "next_actions": next_actions,
        "version": env!("CARGO_PKG_VERSION"),
        "timestamp": now_timestamp_ms(),
    });
    emit(true, &payload)
}

fn now_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
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
    report_file: Option<&str>,
) -> Result<RunReport, SocoreError> {
    metrics::record_connection_start();
    let started = Instant::now();
    match relay::bridge_with_plans(left.clone(), right.clone()).await {
        Ok(stats) => {
            metrics::record_bytes(stats.bytes_left_to_right, stats.bytes_right_to_left);
            let report = RunReport {
                mode,
                profile: profile.map(|p| p.as_str().to_string()),
                from: left,
                to: right,
                stats,
                duration_ms: started.elapsed().as_millis(),
            };
            if let Some(path) = report_file {
                write_json_file(path, &report)?;
            }
            Ok(report)
        }
        Err(err) => {
            metrics::record_connection_failed();
            Err(err)
        }
    }
}

async fn run_check(plan: EndpointPlan, profile: Option<ProfilePreset>) -> CheckReport {
    let started = Instant::now();
    let result = endpoint::open_with_options(plan.endpoint.clone(), &plan.options).await;
    match result {
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
    }
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

fn to_json_value<T: Serialize>(value: &T) -> Result<Value, SocoreError> {
    serde_json::to_value(value).map_err(|e| SocoreError::InvalidAddress(e.to_string()))
}

fn endpoint_plan_to_uri(plan: &EndpointPlan) -> String {
    let mut base = match &plan.endpoint {
        EndpointSpec::Stdio => "stdio://".to_string(),
        EndpointSpec::TcpConnect(addr) => format!("tcp://{addr}"),
        EndpointSpec::TcpListen(addr) => format!("tcp-listen://{addr}"),
        EndpointSpec::UdpConnect(addr) => format!("udp://{addr}"),
        EndpointSpec::UdpListen(addr) => format!("udp-listen://{addr}"),
        EndpointSpec::TlsConnect(addr) => format!("tls://{addr}"),
        EndpointSpec::TlsListen(addr) => format!("tls-listen://{addr}"),
        EndpointSpec::Socks4Connect { proxy, target } => {
            format!("socks4://{proxy}?target={target}")
        }
        EndpointSpec::Socks4aConnect { proxy, target } => {
            format!("socks4a://{proxy}?target={target}")
        }
        EndpointSpec::Socks5Connect {
            proxy,
            target,
            auth,
        } => {
            if let Some(a) = auth {
                format!(
                    "socks5://{}:{}@{proxy}?target={target}",
                    a.username, a.password
                )
            } else {
                format!("socks5://{proxy}?target={target}")
            }
        }
        EndpointSpec::HttpProxyConnect {
            proxy,
            target,
            auth,
        } => {
            if let Some(a) = auth {
                format!(
                    "http-proxy://{}:{}@{proxy}?target={target}",
                    a.username, a.password
                )
            } else {
                format!("http-proxy://{proxy}?target={target}")
            }
        }
        EndpointSpec::ProxyChain { hops, target } => {
            let chain = hops
                .iter()
                .map(|h| format!("{:?}@{}", h.kind, h.proxy))
                .collect::<Vec<_>>()
                .join(",");
            format!("proxy-chain://{chain}?target={target}")
        }
        EndpointSpec::Exec(cmd) => format!("exec://?cmd={cmd}"),
        EndpointSpec::System(cmd) => format!("system://?cmd={cmd}"),
        EndpointSpec::Shell(cmd) => format!("shell://?cmd={cmd}"),
        EndpointSpec::UnixConnect(path) => format!("unix://{}", path.display()),
        EndpointSpec::UnixListen(path) => format!("unix-listen://{}", path.display()),
        EndpointSpec::File(path) => format!("file://{}", path.display()),
        EndpointSpec::NamedPipe(path) => format!("npipe://{}", path.replace('\\', "/")),
        EndpointSpec::Unsupported(name) => format!("unsupported://{name}"),
    };

    let options = endpoint_options_query(&plan.options);
    if !options.is_empty() {
        if base.contains('?') {
            base.push('&');
        } else {
            base.push('?');
        }
        base.push_str(&options.join("&"));
    }
    base
}

fn endpoint_options_query(options: &EndpointOptions) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(v) = options.connect_timeout_ms {
        out.push(format!("connect-timeout={v}ms"));
    }
    if let Some(v) = options.retry {
        out.push(format!("retry={v}"));
    }
    if let Some(v) = options.retry_delay_ms {
        out.push(format!("retry-delay={v}ms"));
    }
    if let Some(v) = options.retry_backoff {
        let value = match v {
            crate::spec::RetryBackoff::Constant => "constant",
            crate::spec::RetryBackoff::Exponential => "exponential",
        };
        out.push(format!("retry-backoff={value}"));
    }
    if let Some(v) = options.retry_max_delay_ms {
        out.push(format!("retry-max-delay={v}ms"));
    }
    if let Some(v) = options.tls_verify {
        out.push(format!("tls-verify={v}"));
    }
    if let Some(v) = &options.tls_sni {
        out.push(format!("tls-sni={v}"));
    }
    if let Some(v) = &options.tls_ca_file {
        out.push(format!("tls-ca-file={v}"));
    }
    if let Some(v) = &options.tls_client_pkcs12 {
        out.push(format!("tls-client-pkcs12={v}"));
    }
    if let Some(v) = &options.tls_client_password {
        out.push(format!("tls-client-password={v}"));
    }
    out
}

fn plan_payload(mode: &str, left: &EndpointPlan, right: &EndpointPlan) -> Value {
    let from = endpoint_plan_to_uri(left);
    let to = endpoint_plan_to_uri(right);
    json!({
        "mode": mode,
        "normalized_endpoints": {
            "from": from,
            "to": to,
        },
        "executable_command": format!("socat link --from {} --to {}", from, to),
    })
}

fn command_name(command: Option<&Command>) -> &'static str {
    match command {
        Some(Command::Link(_)) => "link",
        Some(Command::Tunnel(_)) => "tunnel",
        Some(Command::Plan(_)) => "plan",
        Some(Command::Validate(_)) => "validate",
        Some(Command::Run(_)) => "run",
        Some(Command::Check { .. }) => "check",
        Some(Command::Explain { .. }) => "explain",
        Some(Command::Inventory) => "inventory",
        Some(Command::Doctor) => "doctor",
        None => "legacy",
    }
}

fn build_cli_input_payload(cli: &Cli) -> Value {
    json!({
        "dry_run": cli.dry_run,
        "profile": cli.profile.map(|p| p.as_str()),
        "metrics_bind": cli.metrics_bind,
        "report_file": cli.report_file,
        "legacy": cli.legacy,
    })
}

fn json_error_from_socore_error(err: &SocoreError) -> JsonError {
    let code = error_code(err);
    JsonError {
        code,
        message: err.to_string(),
        hint: error_hint(err).to_string(),
    }
}

fn error_code(err: &SocoreError) -> &'static str {
    match err {
        SocoreError::InvalidAddress(_) => "E_ADDR_PARSE",
        SocoreError::Io(ioe) if ioe.kind() == std::io::ErrorKind::TimedOut => "E_CONNECT_TIMEOUT",
        SocoreError::UnsupportedEndpoint(msg) if msg.contains("tls-listen requires env") => {
            "E_TLS_ENV"
        }
        SocoreError::UnsupportedEndpoint(msg)
            if msg.contains("username/password") || msg.contains("auth") =>
        {
            "E_PROXY_AUTH"
        }
        SocoreError::Tls(_) => "E_TLS",
        SocoreError::Io(_) => "E_IO",
        SocoreError::UnsupportedEndpoint(_) => "E_UNSUPPORTED_ENDPOINT",
    }
}

fn error_hint(err: &SocoreError) -> &'static str {
    match error_code(err) {
        "E_ADDR_PARSE" => "Check address syntax with `socat --json explain <address>`",
        "E_CONNECT_TIMEOUT" => {
            "Increase connect-timeout/retry options, then run `socat --json check <address>`"
        }
        "E_TLS_ENV" => {
            "Set SOCAT_RS_TLS_PKCS12 and optional SOCAT_RS_TLS_PASSWORD before tls-listen"
        }
        "E_PROXY_AUTH" => {
            "Provide proxy credentials in URI (e.g. socks5://user:pass@proxy:1080?target=host:443)"
        }
        _ => "Inspect `error.message` and rerun with `--json` for machine-readable diagnostics",
    }
}

fn error_hint_action(err: &SocoreError) -> String {
    error_hint(err).to_string()
}

#[cfg(test)]
mod tests {
    use crate::spec::{EndpointOptions, EndpointSpec};

    use super::{
        EndpointPlan, JSON_SCHEMA_VERSION, ProfilePreset, apply_profile, build_tunnel_endpoint,
        default_release_targets, endpoint_plan_to_uri, ensure_tunnel_via_has_target,
        parse_endpoint_plan,
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
    fn doctor_targets_include_major_platforms() {
        let targets = default_release_targets();
        assert!(targets.contains(&"x86_64-unknown-linux-gnu"));
        assert!(targets.contains(&"aarch64-unknown-linux-gnu"));
        assert!(targets.contains(&"x86_64-pc-windows-msvc"));
        assert!(targets.contains(&"aarch64-pc-windows-msvc"));
        assert!(targets.contains(&"x86_64-apple-darwin"));
        assert!(targets.contains(&"aarch64-apple-darwin"));
    }

    #[test]
    fn endpoint_uri_contains_runtime_options() {
        let plan = EndpointPlan {
            endpoint: EndpointSpec::TcpConnect("127.0.0.1:9000".to_string()),
            options: EndpointOptions {
                connect_timeout_ms: Some(1000),
                retry: Some(2),
                retry_delay_ms: Some(50),
                retry_backoff: None,
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
                tls_ca_file: None,
                tls_client_pkcs12: None,
                tls_client_password: None,
            },
        };
        let uri = endpoint_plan_to_uri(&plan);
        assert!(uri.contains("connect-timeout=1000ms"));
        assert!(uri.contains("retry=2"));
        assert!(uri.contains("retry-delay=50ms"));
    }

    #[test]
    fn schema_version_stable() {
        assert_eq!(JSON_SCHEMA_VERSION, "1.0.0");
    }
}
