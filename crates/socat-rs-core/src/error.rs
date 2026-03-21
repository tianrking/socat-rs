use thiserror::Error;

#[derive(Debug, Error)]
pub enum SocoreError {
    #[error("invalid address syntax: {0}")]
    InvalidAddress(String),
    #[error("unsupported endpoint: {0}")]
    UnsupportedEndpoint(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("tls error: {0}")]
    Tls(#[from] native_tls::Error),
}
