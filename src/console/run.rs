use super::{
    cli::ConsoleCommand,
    endpoint::resolve_endpoint,
    exec::run_exec,
    repl::{history_file_path, run_repl},
    rpc::RpcClient,
};
use eyre::Result;
use serde_json::Value;
use std::collections::BTreeMap;

pub async fn run_console(cmd: ConsoleCommand) -> Result<()> {
    let endpoint = resolve_endpoint(cmd.endpoint.as_deref())?;
    let rpc = RpcClient::connect(&endpoint).await?;

    let chain_id =
        rpc.request_value("eth_chainId", None).await.ok().and_then(|v| parse_chain_id(&v));

    let bera_admin_status = rpc.request_value("beraAdmin_nodeStatus", None).await.ok();
    let has_bera_admin = bera_admin_status.is_some();

    let mut rpc_aliases = default_aliases();
    if has_bera_admin {
        for (alias, method) in [
            ("peers", "beraAdmin_detailedPeers"),
            ("status", "beraAdmin_nodeStatus"),
            ("ban", "beraAdmin_banPeer"),
            ("penalize", "beraAdmin_penalizePeer"),
        ] {
            rpc_aliases.insert(alias.to_owned(), method.to_owned());
        }
    }

    if let Some(script) = cmd.exec.as_deref() {
        run_exec(&rpc, script, &rpc_aliases).await?;
    } else {
        run_repl(
            &rpc,
            history_file_path(),
            endpoint,
            &rpc_aliases,
            chain_id,
            cmd.raw,
            has_bera_admin,
            bera_admin_status,
        )
        .await?;
    }

    Ok(())
}

fn default_aliases() -> BTreeMap<String, String> {
    [
        ("eth.blockNumber", "eth_blockNumber"),
        ("net.version", "net_version"),
        ("web3.clientVersion", "web3_clientVersion"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

fn parse_chain_id(value: &Value) -> Option<u64> {
    match value {
        Value::String(s) => {
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else {
                s.parse().ok()
            }
        }
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}
