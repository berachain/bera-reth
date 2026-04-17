use super::{
    command::{InputCommand, parse_input},
    query::apply_query,
    rpc::RpcClient,
};
use eyre::{Result, eyre};
use serde_json::Value;

pub enum EvalOutcome {
    Noop,
    Exit,
    Help,
    Value(Value),
}

pub async fn evaluate_line(
    rpc: &RpcClient,
    line: &str,
    last_rpc_result: &mut Option<Value>,
) -> Result<EvalOutcome> {
    match parse_input(line)? {
        InputCommand::Empty => Ok(EvalOutcome::Noop),
        InputCommand::Exit => Ok(EvalOutcome::Exit),
        InputCommand::Help => Ok(EvalOutcome::Help),
        InputCommand::Query(expr) => {
            let next = apply_query_to_last_rpc(&expr, last_rpc_result)?;
            Ok(EvalOutcome::Value(next))
        }
        InputCommand::Rpc { method, params } => {
            let normalized_method = normalize_rpc_method(&method);
            let value = rpc.request_value(&normalized_method, params).await?;
            *last_rpc_result = Some(value.clone());
            Ok(EvalOutcome::Value(value))
        }
        InputCommand::RpcWithQuery { method, params, query } => {
            let normalized_method = normalize_rpc_method(&method);
            let value = rpc.request_value(&normalized_method, params).await?;
            *last_rpc_result = Some(value.clone());
            let result = apply_query(&query, &value)?;
            Ok(EvalOutcome::Value(result))
        }
        InputCommand::MethodToken(token) => {
            if is_remove_all_peers_token(token.as_str()) {
                return run_remove_all_peers(rpc, last_rpc_result).await;
            }
            let method = normalize_rpc_method(&token);
            let value = rpc.request_value(&method, None).await?;
            *last_rpc_result = Some(value.clone());
            Ok(EvalOutcome::Value(value))
        }
    }
}

fn is_remove_all_peers_token(s: &str) -> bool {
    matches!(s, "removeAllPeers" | "admin.removeAllPeers")
}

async fn run_remove_all_peers(
    rpc: &RpcClient,
    last_rpc_result: &mut Option<Value>,
) -> Result<EvalOutcome> {
    let peers = rpc.request_value("admin_peers", None).await?;
    let arr = peers.as_array().ok_or_else(|| eyre!("admin.peers did not return an array"))?;
    let total = arr.len() as u64;
    let mut removed = 0u64;
    let mut failed = 0u64;
    for peer in arr {
        let enode = peer
            .get("enode")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("peer entry missing enode"))?;
        match rpc.request_value("admin_removePeer", Some(serde_json::json!([enode]))).await {
            Ok(Value::Bool(true)) => removed += 1,
            Ok(other) => {
                eprintln!("admin.removePeer({enode}): unexpected response: {other}");
                failed += 1;
            }
            Err(err) => {
                eprintln!("admin.removePeer({enode}): {err}");
                failed += 1;
            }
        }
    }
    *last_rpc_result = Some(peers.clone());
    eprintln!("Removed {removed}/{total} peer(s).");
    Ok(EvalOutcome::Value(serde_json::json!({
        "removed": removed,
        "failed": failed,
        "total": total
    })))
}

fn apply_query_to_last_rpc(expr: &str, last_rpc_result: &Option<Value>) -> Result<Value> {
    let value =
        last_rpc_result.as_ref().ok_or_else(|| eyre!("no last rpc result available for query"))?;
    apply_query(expr, value)
}

fn normalize_rpc_method(method: &str) -> String {
    method.replace('.', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_rpc_methods() {
        assert_eq!(normalize_rpc_method("eth.getBalance"), "eth_getBalance");
    }

    #[test]
    fn query_without_last_result_errors() {
        let err = apply_query_to_last_rpc(".count", &None).unwrap_err();
        assert!(err.to_string().contains("no last rpc result available for query"));
    }

    #[test]
    fn apply_query_on_array_returns_count() {
        let peers = json!([{"id": "a"}, {"id": "b"}]);
        let count = apply_query(".count", &peers).unwrap();
        assert_eq!(count, json!(2));
    }
}
