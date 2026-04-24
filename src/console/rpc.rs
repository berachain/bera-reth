use super::endpoint::{ResolvedEndpoint, Transport};
use eyre::{Result, eyre};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    ffi::CString,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

#[derive(Debug)]
pub enum RpcClient {
    Ipc(IpcClientLite),
}

impl RpcClient {
    pub async fn connect(endpoint: &ResolvedEndpoint) -> Result<Self> {
        match endpoint.transport {
            Transport::Ipc => {
                validate_ipc_endpoint(&endpoint.raw)?;
                let client = IpcClientLite::new(endpoint.raw.clone());
                Ok(Self::Ipc(client))
            }
        }
    }

    pub async fn request_value(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let params = RpcParams::from_value(params)?;
        match self {
            Self::Ipc(client) => client.request(method, params.into_value()).await,
        }
    }

    pub async fn supported_modules(&self) -> Result<BTreeMap<String, String>> {
        let value = self.request_value("rpc_modules", None).await?;
        serde_json::from_value(value)
            .map_err(|e| eyre!("failed to parse rpc_modules response as a JSON object: {e}"))
    }
}

enum RpcParams {
    None,
    Array(Vec<Value>),
    Object(Map<String, Value>),
}

impl RpcParams {
    fn from_value(value: Option<Value>) -> Result<Self> {
        let Some(value) = value else {
            return Ok(Self::None);
        };
        match value {
            Value::Null => Ok(Self::None),
            Value::Array(values) => Ok(Self::Array(values)),
            Value::Object(values) => Ok(Self::Object(values)),
            _ => Err(eyre!("rpc params must be null, JSON array, or JSON object")),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::None => Value::Array(vec![]),
            Self::Array(values) => Value::Array(values),
            Self::Object(values) => Value::Object(values),
        }
    }
}

fn validate_ipc_endpoint(path: &str) -> Result<()> {
    let endpoint = Path::new(path);
    if !endpoint.exists() {
        return Err(eyre!("IPC endpoint not found: {path}"));
    }
    let metadata = std::fs::metadata(endpoint)
        .map_err(|err| eyre!("failed to stat IPC endpoint {path}: {err}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if !metadata.file_type().is_socket() {
            return Err(eyre!("IPC endpoint is not a unix socket: {path}"));
        }
        let c_path =
            CString::new(path).map_err(|_| eyre!("IPC endpoint contains invalid bytes: {path}"))?;
        let read_ok = unsafe { libc::access(c_path.as_ptr(), libc::R_OK) == 0 };
        if !read_ok {
            return Err(eyre!("IPC endpoint is not readable by current user: {path}"));
        }
        let write_ok = unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 };
        if !write_ok {
            return Err(eyre!("IPC endpoint is not writable by current user: {path}"));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct IpcClientLite {
    path: String,
    next_id: AtomicU64,
}

impl IpcClientLite {
    fn new(path: String) -> Self {
        Self { path, next_id: AtomicU64::new(1) }
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        const IPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        tokio::time::timeout(IPC_TIMEOUT, self.request_inner(method, params))
            .await
            .map_err(|_| eyre!("IPC request timed out after {IPC_TIMEOUT:?}"))?
    }

    async fn request_inner(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut stream = UnixStream::connect(&self.path)
            .await
            .map_err(|err| eyre!("failed to connect IPC endpoint {}: {err}", self.path))?;
        let encoded = serde_json::to_string(&req)?;
        stream.write_all(encoded.as_bytes()).await?;
        stream.write_all(b"\n").await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            return Err(eyre!("empty IPC response"));
        }

        let resp: Value = serde_json::from_str(&line)?;
        if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
            return Err(eyre!("rpc error: {}", err));
        }
        resp.get("result").cloned().ok_or_else(|| eyre!("missing result field in IPC response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };

    #[test]
    fn rpc_params_accept_none_and_null() {
        let none_params = RpcParams::from_value(None).unwrap();
        assert!(matches!(none_params, RpcParams::None));

        let null_params = RpcParams::from_value(Some(Value::Null)).unwrap();
        assert!(matches!(null_params, RpcParams::None));
    }

    #[test]
    fn rpc_params_reject_scalar_values() {
        let err = match RpcParams::from_value(Some(json!(true))) {
            Ok(_) => panic!("expected scalar params to be rejected"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("rpc params must be null, JSON array, or JSON object"));
    }

    #[test]
    fn rpc_params_preserve_array_and_object_shapes() {
        let array = RpcParams::from_value(Some(json!([1, "x"]))).unwrap();
        assert_eq!(array.into_value(), json!([1, "x"]));

        let object = RpcParams::from_value(Some(json!({"a": 1, "b": "x"}))).unwrap();
        assert_eq!(object.into_value(), json!({"a": 1, "b": "x"}));
    }

    #[test]
    fn validate_ipc_endpoint_errors_for_missing_and_non_socket() {
        let missing = validate_ipc_endpoint("/definitely/missing/reth.ipc").unwrap_err();
        assert!(missing.to_string().contains("IPC endpoint not found"));

        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("plain-file");
        std::fs::write(&file_path, b"not a socket").expect("write file");
        let err = validate_ipc_endpoint(file_path.to_string_lossy().as_ref()).unwrap_err();
        assert!(err.to_string().contains("not a unix socket"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ipc_client_handles_empty_response_and_rpc_error() {
        let dir = tempdir().expect("tempdir");
        let socket_path = dir.path().join("reth.ipc");
        let listener = UnixListener::bind(&socket_path).expect("bind socket");

        let server_task = tokio::spawn(async move {
            // First request: reply with an empty line.
            let (stream1, _) = listener.accept().await.expect("accept first");
            let mut r1 = BufReader::new(stream1);
            let mut req1 = String::new();
            let _ = r1.read_line(&mut req1).await.expect("read first");
            let mut s1 = r1.into_inner();
            s1.write_all(b"\n").await.expect("write empty response");

            // Second request: reply with JSON-RPC error.
            let (stream2, _) = listener.accept().await.expect("accept second");
            let mut r2 = BufReader::new(stream2);
            let mut req2 = String::new();
            let _ = r2.read_line(&mut req2).await.expect("read second");
            let mut s2 = r2.into_inner();
            s2.write_all(br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"boom"}}"#)
                .await
                .expect("write error");
            s2.write_all(b"\n").await.expect("write newline");
        });

        let client = IpcClientLite::new(socket_path.to_string_lossy().to_string());
        let empty_err = client.request("eth_blockNumber", json!([])).await.unwrap_err();
        assert!(empty_err.to_string().contains("empty IPC response"));

        let rpc_err = client.request("eth_blockNumber", json!([])).await.unwrap_err();
        assert!(rpc_err.to_string().contains("rpc error"));

        server_task.await.expect("server task");
    }
}
