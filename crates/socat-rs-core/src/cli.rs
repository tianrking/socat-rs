use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::spec::{EndpointOptions, RetryBackoff};

#[derive(Debug, Parser)]
#[command(name = "socat")]
#[command(about = "Modern socat rewrite: compatibility path + simple path")]
pub(crate) struct Cli {
    #[arg(global = true, long, help = "Print machine-readable errors/info")]
    pub(crate) json: bool,
    #[arg(global = true, long, help = "Only parse and show resolved plan")]
    pub(crate) dry_run: bool,
    #[arg(
        global = true,
        long,
        value_enum,
        help = "Apply built-in connection profile defaults"
    )]
    pub(crate) profile: Option<ProfilePreset>,
    #[arg(
        global = true,
        long,
        help = "Expose Prometheus metrics on host:port (e.g. 0.0.0.0:9464)"
    )]
    pub(crate) metrics_bind: Option<String>,
    #[arg(global = true, long, help = "Write JSON run report to file path")]
    pub(crate) report_file: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
    #[arg(value_name = "ADDRESS", num_args = 0..=2)]
    pub(crate) legacy: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Link(LinkArgs),
    Tunnel(TunnelArgs),
    Plan(LinkArgs),
    Validate(LinkArgs),
    Check { address: String },
    Explain { address: String },
    Inventory,
    Doctor,
}

#[derive(Debug, Args)]
pub(crate) struct LinkArgs {
    #[arg(long)]
    pub(crate) from: String,
    #[arg(long)]
    pub(crate) to: String,
}

#[derive(Debug, Args)]
pub(crate) struct TunnelArgs {
    #[arg(long, default_value = "stdio://")]
    pub(crate) from: String,
    #[arg(long, required = true, num_args = 1.., action = clap::ArgAction::Append)]
    pub(crate) via: Vec<String>,
    #[arg(long, help = "final tunnel target in host:port form")]
    pub(crate) to: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum ProfilePreset {
    Dev,
    Prod,
    Lan,
    Wan,
}

impl ProfilePreset {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Prod => "prod",
            Self::Lan => "lan",
            Self::Wan => "wan",
        }
    }

    pub(crate) fn defaults(self) -> EndpointOptions {
        match self {
            Self::Dev => EndpointOptions {
                connect_timeout_ms: Some(3_000),
                retry: Some(1),
                retry_delay_ms: Some(200),
                retry_backoff: Some(RetryBackoff::Constant),
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
                tls_ca_file: None,
                tls_client_pkcs12: None,
                tls_client_password: None,
            },
            Self::Prod => EndpointOptions {
                connect_timeout_ms: Some(5_000),
                retry: Some(5),
                retry_delay_ms: Some(500),
                retry_backoff: Some(RetryBackoff::Exponential),
                retry_max_delay_ms: Some(10_000),
                tls_verify: Some(true),
                tls_sni: None,
                tls_ca_file: None,
                tls_client_pkcs12: None,
                tls_client_password: None,
            },
            Self::Lan => EndpointOptions {
                connect_timeout_ms: Some(1_500),
                retry: Some(2),
                retry_delay_ms: Some(100),
                retry_backoff: Some(RetryBackoff::Constant),
                retry_max_delay_ms: None,
                tls_verify: None,
                tls_sni: None,
                tls_ca_file: None,
                tls_client_pkcs12: None,
                tls_client_password: None,
            },
            Self::Wan => EndpointOptions {
                connect_timeout_ms: Some(10_000),
                retry: Some(4),
                retry_delay_ms: Some(1_000),
                retry_backoff: Some(RetryBackoff::Exponential),
                retry_max_delay_ms: Some(15_000),
                tls_verify: Some(true),
                tls_sni: None,
                tls_ca_file: None,
                tls_client_pkcs12: None,
                tls_client_password: None,
            },
        }
    }
}
