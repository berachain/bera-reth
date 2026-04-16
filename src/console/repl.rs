use super::{
    endpoint::{ResolvedEndpoint, default_datadir},
    engine::{EvalOutcome, evaluate_line},
    output::{BERACHAIN_CHAIN_IDS, hex_or_decimal_to_u64, print_value_for_chain_raw},
    rpc::RpcClient,
};
use eyre::Result;
use rustyline::{
    CompletionType, Context, Editor, Helper,
    completion::{Completer, Pair},
    config::Configurer,
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    history::DefaultHistory,
    validate::Validator,
};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[allow(clippy::too_many_arguments)]
pub async fn run_repl(
    rpc: &RpcClient,
    history_path: PathBuf,
    endpoint: ResolvedEndpoint,
    aliases: &BTreeMap<String, String>,
    chain_id: Option<u64>,
    raw: bool,
    has_bera_admin: bool,
    bera_admin_status: Option<Value>,
) -> Result<()> {
    std::fs::create_dir_all(
        history_path.parent().map(ToOwned::to_owned).unwrap_or_else(|| PathBuf::from(".")),
    )?;

    let modules = rpc.supported_modules().await.unwrap_or_default();
    let helper = CompletionHelper::new(aliases, &modules, has_bera_admin);
    let mut editor: Editor<CompletionHelper, DefaultHistory> = Editor::new()?;
    editor.set_completion_type(CompletionType::List);
    editor.set_helper(Some(helper));
    if history_path.exists() {
        let _ = editor.load_history(&history_path);
    }

    println!("bera-reth console :: {}", endpoint.raw);
    print_startup_snapshot(rpc, chain_id, bera_admin_status.as_ref()).await;
    println!("help: commands | ctrl-d / exit: quit");

    let mut last_rpc_result = None;
    loop {
        let line = editor.readline("bera> ");
        match line {
            Ok(line) => {
                if !line.trim().is_empty() {
                    let _ = editor.add_history_entry(line.as_str());
                }
                match evaluate_line(rpc, aliases, &line, &mut last_rpc_result).await {
                    Ok(EvalOutcome::Noop) => {}
                    Ok(EvalOutcome::Exit) => break,
                    Ok(EvalOutcome::Help) => print_help(aliases, has_bera_admin),
                    Ok(EvalOutcome::Value(value)) => {
                        print_value_for_chain_raw(&value, chain_id, raw);
                    }
                    Err(err) => eprintln!("error: {err}"),
                }
            }
            Err(ReadlineError::Interrupted) => {}
            Err(ReadlineError::Eof) => break,
            Err(err) => return Err(err.into()),
        }
    }

    let _ = editor.save_history(&history_path);
    Ok(())
}

async fn print_startup_snapshot(
    rpc: &RpcClient,
    chain_id: Option<u64>,
    bera_admin_status: Option<&Value>,
) {
    if let Some(status) = bera_admin_status {
        let client_version = status.get("client").and_then(as_string);
        let network_id = status.get("networkId").and_then(as_string);
        let head_number = status.get("head").and_then(as_string);
        let peer_count_total = status.get("peerCountTotal").and_then(hex_or_decimal_to_u64);
        let peer_count_inbound = status.get("peerCountInbound").and_then(hex_or_decimal_to_u64);
        let peer_count_outbound = status.get("peerCountOutbound").and_then(hex_or_decimal_to_u64);

        let peers_str = if let (Some(in_count), Some(out_count)) =
            (peer_count_inbound, peer_count_outbound)
        {
            format!("peers={} (in={} out={})", peer_count_total.unwrap_or(0), in_count, out_count)
        } else {
            format!("peers={}", peer_count_total.unwrap_or(0))
        };

        println!(
            "node :: {} | net={} 🐻⭐ | block={} | {}",
            client_version.unwrap_or_else(|| "unavailable".to_owned()),
            network_id.unwrap_or_else(|| "unavailable".to_owned()),
            head_number.unwrap_or_else(|| "unavailable".to_owned()),
            peers_str
        );
    } else {
        let version =
            rpc.request_value("web3_clientVersion", None).await.ok().and_then(|v| as_string(&v));
        let block = rpc
            .request_value("eth_blockNumber", None)
            .await
            .ok()
            .and_then(|v| hex_or_decimal_to_u64(&v).map(|n| n.to_string()));
        let peers = rpc
            .request_value("net_peerCount", None)
            .await
            .ok()
            .and_then(|v| hex_or_decimal_to_u64(&v).map(|n| n.to_string()));
        let network = rpc.request_value("net_version", None).await.ok().and_then(|v| as_string(&v));

        println!(
            "node :: version={} | net={}{} | block={} | peers={}",
            version.unwrap_or_else(|| "unavailable".to_owned()),
            network.unwrap_or_else(|| "unavailable".to_owned()),
            chain_emoji(chain_id),
            block.unwrap_or_else(|| "unavailable".to_owned()),
            peers.unwrap_or_else(|| "unavailable".to_owned()),
        );
    }
}

fn chain_emoji(chain_id: Option<u64>) -> &'static str {
    match chain_id {
        Some(id) if BERACHAIN_CHAIN_IDS.contains(&id) => " 🐻",
        _ => "",
    }
}

fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn print_help(aliases: &BTreeMap<String, String>, has_bera_admin: bool) {
    println!("Commands:");
    println!("  <method> [json_params]   (RPC call)");
    println!("  <alias>                  (e.g. eth.blockNumber)");
    println!("  TAB                      completion for aliases/methods");
    println!("  help | exit");
    println!("Queries (run against last RPC result):");
    println!("  .count | .len | .first | .last | .[0] | .[0].field | .map(.field)");
    if has_bera_admin {
        println!("beraAdmin (when detected):");
        println!("  peers                 detailed peer table");
        println!("  status                node identity and sync state");
        println!("  ban \"0xpeerId\"        ban peer (~12h)");
        println!("  penalize \"0xpeerId\" -100   penalize peer by value");
    }
    if !aliases.is_empty() {
        println!("Aliases:");
        for (alias, method) in aliases {
            println!("  {alias} -> {method}");
        }
    }
}

struct CompletionHelper {
    words: Vec<String>,
}

impl CompletionHelper {
    fn new(
        aliases: &BTreeMap<String, String>,
        modules: &BTreeMap<String, String>,
        has_bera_admin: bool,
    ) -> CompletionHelper {
        let mut words = vec![
            "help".to_owned(),
            "exit".to_owned(),
            "quit".to_owned(),
            ".count".to_owned(),
            ".len".to_owned(),
            ".first".to_owned(),
            ".last".to_owned(),
            ".map(".to_owned(),
        ];
        words.extend(aliases.keys().cloned());
        for method in aliases.values() {
            words.push(method.clone());
            if let Some(dot) = rpc_method_to_dot(method) {
                words.push(dot);
            }
        }
        for module in modules.keys() {
            words.push(format!("{module}."));
            words.push(format!("{module}_"));
            words.extend(super::rpc_completion::dot_completions_for_namespace(module));
        }
        if has_bera_admin {
            words.push("beraAdmin.".to_owned());
            words.push("beraAdmin_".to_owned());
            words.extend(super::rpc_completion::dot_completions_for_namespace("beraAdmin"));
        }
        words.sort();
        words.dedup();
        CompletionHelper { words }
    }
}

fn rpc_method_to_dot(method: &str) -> Option<String> {
    let (module, rest) = method.split_once('_')?;
    if module.is_empty() || rest.is_empty() {
        return None;
    }
    Some(format!("{module}.{rest}"))
}

impl Helper for CompletionHelper {}
impl Validator for CompletionHelper {}
impl Highlighter for CompletionHelper {}
impl Hinter for CompletionHelper {
    type Hint = String;
}

impl Completer for CompletionHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let safe_pos = pos.min(line.len());
        let up_to_cursor = &line[..safe_pos];
        let start = up_to_cursor
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        let needle = &up_to_cursor[start..];
        let matches = self
            .words
            .iter()
            .filter(|word| word.starts_with(needle))
            .map(|word| Pair { display: word.clone(), replacement: word.clone() })
            .collect();
        Ok((start, matches))
    }
}

pub fn history_file_path() -> PathBuf {
    default_datadir().join("bera-reth-console-history")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::{completion::Completer, history::DefaultHistory};
    use serde_json::json;

    #[test]
    fn completion_includes_bera_admin_when_enabled() {
        let aliases = BTreeMap::new();
        let modules = BTreeMap::new();
        let helper = CompletionHelper::new(&aliases, &modules, true);
        assert!(helper.words.iter().any(|w| w == "beraAdmin.detailedPeers"));
    }

    #[test]
    fn completion_excludes_bera_admin_when_disabled() {
        let aliases = BTreeMap::new();
        let modules = BTreeMap::new();
        let helper = CompletionHelper::new(&aliases, &modules, false);
        assert!(!helper.words.iter().any(|w| w == "beraAdmin.detailedPeers"));
    }

    #[test]
    fn completion_matches_prefix() {
        let aliases = BTreeMap::from([("bn".to_owned(), "eth_blockNumber".to_owned())]);
        let modules = BTreeMap::from([("eth".to_owned(), "1.0".to_owned())]);
        let helper = CompletionHelper::new(&aliases, &modules, false);
        let history = DefaultHistory::new();
        let ctx = Context::new(&history);
        let (_start, hits) = helper.complete("eth.getB", "eth.getB".len(), &ctx).unwrap();
        assert!(hits.iter().any(|p| p.replacement == "eth.getBalance"));
    }

    #[test]
    fn completion_includes_eth_namespace_methods() {
        let aliases = BTreeMap::new();
        let modules = BTreeMap::from([("eth".to_owned(), "1.0".to_owned())]);
        let helper = CompletionHelper::new(&aliases, &modules, false);
        assert!(helper.words.iter().any(|w| w == "eth.getLogs"));
        assert!(helper.words.iter().any(|w| w == "eth.getTransactionReceipt"));
    }

    #[test]
    fn parses_hex_or_decimal_numbers() {
        assert_eq!(hex_or_decimal_to_u64(&json!("0x10")), Some(16));
    }
}
