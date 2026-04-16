use eyre::{Result, bail};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Http,
    Ws,
    Ipc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEndpoint {
    pub raw: String,
    pub transport: Transport,
}

const DEFAULT_IPC_FILENAME: &str = "reth.ipc";

pub fn default_datadir() -> PathBuf {
    if cfg!(target_os = "macos") &&
        let Some(home) = dirs::home_dir()
    {
        return home.join("Library").join("Application Support").join("reth");
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("reth")
}

pub fn resolve_endpoint(endpoint: Option<&str>) -> Result<ResolvedEndpoint> {
    let raw = match endpoint {
        Some(e) => e.to_owned(),
        None => default_datadir().join(DEFAULT_IPC_FILENAME).to_string_lossy().into_owned(),
    };
    let transport = detect_transport(&raw)?;
    Ok(ResolvedEndpoint { raw, transport })
}

fn detect_transport(endpoint: &str) -> Result<Transport> {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return Ok(Transport::Http);
    }
    if endpoint.starts_with("ws://") || endpoint.starts_with("wss://") {
        return Ok(Transport::Ws);
    }
    if endpoint.contains("://") {
        bail!("unsupported endpoint scheme in {endpoint:?}");
    }
    Ok(Transport::Ipc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_ipc_path() {
        let got = resolve_endpoint(None).unwrap();
        assert_eq!(got.transport, Transport::Ipc);
        assert!(got.raw.ends_with("reth.ipc"));
    }

    #[test]
    fn parses_http() {
        let got = resolve_endpoint(Some("http://127.0.0.1:8545")).unwrap();
        assert_eq!(got.transport, Transport::Http);
    }

    #[test]
    fn parses_ws() {
        let got = resolve_endpoint(Some("ws://127.0.0.1:8546")).unwrap();
        assert_eq!(got.transport, Transport::Ws);
    }

    #[test]
    fn rejects_unknown_scheme() {
        let err = resolve_endpoint(Some("ftp://example.test")).unwrap_err();
        assert!(err.to_string().contains("unsupported endpoint scheme"));
    }
}
