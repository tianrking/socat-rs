mod endpoint;
mod error;
mod relay;
mod spec;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use crate::error::SocoreError;
use crate::spec::EndpointPlan;

#[derive(Debug, Parser)]
#[command(name = "socat")]
#[command(about = "Modern socat rewrite: compatibility path + simple path")]
struct Cli {
    #[arg(global = true, long, help = "Print machine-readable errors/info")]
    json: bool,
    #[arg(global = true, long, help = "Only parse and show resolved plan")]
    dry_run: bool,
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(value_name = "ADDRESS", num_args = 0..=2)]
    legacy: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Link(LinkArgs),
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
    from: EndpointPlan,
    to: EndpointPlan,
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
    match (cli.command, cli.legacy.as_slice()) {
        (Some(Command::Inventory), _) => {
            print_inventory(cli.json);
            Ok(())
        }
        (Some(Command::Explain { address }), _) => {
            let plan = if address.contains("://") {
                spec::parse_simple_uri_with_options(&address)?
            } else {
                spec::parse_legacy_with_options(&address)?
            };
            emit(cli.json, &plan)?;
            Ok(())
        }
        (Some(Command::Link(args)), _) => {
            let left = spec::parse_simple_uri_with_options(&args.from)?;
            let right = spec::parse_simple_uri_with_options(&args.to)?;
            if cli.dry_run {
                let out = PlanOutput {
                    mode: "simple",
                    from: left,
                    to: right,
                };
                emit(cli.json, &out)?;
                return Ok(());
            }
            relay::bridge_with_plans(left, right).await
        }
        (None, [left, right]) => {
            let left = spec::parse_legacy_with_options(left)?;
            let right = spec::parse_legacy_with_options(right)?;
            if cli.dry_run {
                let out = PlanOutput {
                    mode: "legacy",
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
    }

    let out = Inventory {
        legacy_address_keywords: socat_rs_compat::LEGACY_ADDRESS_KEYWORDS,
        legacy_option_keywords: socat_rs_compat::LEGACY_OPTION_KEYWORDS,
        legacy_address_handlers: socat_rs_compat::LEGACY_ADDRESS_HANDLERS,
        implemented_modes: socat_rs_compat::COMPAT_MODES,
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
