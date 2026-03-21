mod endpoint;
mod error;
mod relay;
mod spec;

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::error::SocoreError;
use crate::spec::{EndpointOptions, EndpointPlan};

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
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(value_name = "ADDRESS", num_args = 0..=2)]
    legacy: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Link(LinkArgs),
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

#[derive(Debug, Serialize)]
struct PlanOutput {
    mode: &'static str,
    profile: Option<String>,
    valid: bool,
    from: EndpointPlan,
    to: EndpointPlan,
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
            },
            Self::Prod => EndpointOptions {
                connect_timeout_ms: Some(5_000),
                retry: Some(5),
                retry_delay_ms: Some(500),
            },
            Self::Lan => EndpointOptions {
                connect_timeout_ms: Some(1_500),
                retry: Some(2),
                retry_delay_ms: Some(100),
            },
            Self::Wan => EndpointOptions {
                connect_timeout_ms: Some(10_000),
                retry: Some(4),
                retry_delay_ms: Some(1_000),
            },
        }
    }
}

pub fn run() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;

    let result = runtime.block_on(async move { dispatch(cli).await });

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
            relay::bridge_with_plans(left, right).await
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
            relay::bridge_with_plans(left, right).await
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

#[cfg(test)]
mod tests {
    use crate::spec::{EndpointOptions, EndpointSpec};

    use super::{EndpointPlan, ProfilePreset, apply_profile, parse_endpoint_plan};

    #[test]
    fn profile_fills_missing_options() {
        let plan = EndpointPlan {
            endpoint: EndpointSpec::TcpConnect("127.0.0.1:80".to_string()),
            options: EndpointOptions {
                connect_timeout_ms: None,
                retry: Some(9),
                retry_delay_ms: None,
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
}
