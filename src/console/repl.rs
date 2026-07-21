use super::{
    endpoint::{ResolvedEndpoint, default_datadir},
    engine::{EvalOutcome, evaluate_line},
    output::{BERACHAIN_CHAIN_IDS, hex_or_decimal_to_u64, print_value_for_chain_raw},
    rpc::RpcClient,
};
use eyre::Result;
use reedline::{
    ColumnarMenu, Completer, Emacs, FileBackedHistory, HISTORY_SIZE, KeyCode, KeyModifiers,
    MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus, Reedline,
    ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion, default_emacs_keybindings,
};
use serde_json::Value;
use std::{borrow::Cow, collections::BTreeMap, path::PathBuf};

const COMPLETION_MENU_NAME: &str = "completion_menu";

fn completion_keybindings() -> reedline::Keybindings {
    let mut kb = default_emacs_keybindings();
    kb.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu(COMPLETION_MENU_NAME.to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    kb
}

/// Single-line prompt matching the previous `rustyline` `readline("bera> ")` UX.
struct BeraPrompt;

impl Prompt for BeraPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("bera> ")
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _prompt_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed(".. ")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!("({}reverse-search: {}) ", prefix, history_search.term))
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_repl(
    rpc: &RpcClient,
    history_path: PathBuf,
    endpoint: ResolvedEndpoint,
    chain_id: Option<u64>,
    raw: bool,
    has_bera_admin: bool,
    bera_admin_status: Option<Value>,
) -> Result<()> {
    std::fs::create_dir_all(
        history_path.parent().map(ToOwned::to_owned).unwrap_or_else(|| PathBuf::from(".")),
    )?;

    let modules = rpc.supported_modules().await.unwrap_or_default();
    let helper = CompletionHelper::new(&modules, has_bera_admin);

    let history = FileBackedHistory::with_file(HISTORY_SIZE, history_path.clone())
        .map_err(|e| eyre::eyre!("failed to load console history: {e}"))?;

    let completion_menu = Box::new(ColumnarMenu::default().with_name(COMPLETION_MENU_NAME));
    let edit_mode = Box::new(Emacs::new(completion_keybindings()));

    let mut editor = Reedline::create()
        .with_history(Box::new(history))
        .with_completer(Box::new(helper))
        .with_menu(ReedlineMenu::EngineCompleter(completion_menu))
        .with_edit_mode(edit_mode)
        .with_quick_completions(true);

    let prompt = BeraPrompt;

    println!("bera-reth console :: {}", endpoint.raw);
    print_startup_snapshot(rpc, chain_id, bera_admin_status.as_ref()).await;
    println!("help: commands | ctrl-d / exit: quit");

    let mut last_rpc_result = None;
    loop {
        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                match evaluate_line(rpc, &line, &mut last_rpc_result).await {
                    Ok(EvalOutcome::Noop) => {}
                    Ok(EvalOutcome::Exit) => break,
                    Ok(EvalOutcome::Help) => print_help(),
                    Ok(EvalOutcome::Value(value)) => {
                        print_value_for_chain_raw(&value, chain_id, raw);
                    }
                    Err(err) => eprintln!("error: {err}"),
                }
            }
            Ok(Signal::CtrlC) => {}
            Ok(Signal::CtrlD) => break,
            Ok(_) => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

fn format_beradmin_startup_line(status: &Value, chain_id: Option<u64>) -> String {
    let client_version =
        status.get("clientVersion").or_else(|| status.get("client_version")).and_then(as_string);
    let network_id =
        status.get("networkId").or_else(|| status.get("network_id")).and_then(as_string);
    let head_number = status
        .get("headNumber")
        .or_else(|| status.get("head_number"))
        .and_then(|v| hex_or_decimal_to_u64(v).map(|n| n.to_string()))
        .or_else(|| status.get("head").and_then(as_string));
    let peer_count_total = status
        .get("peerCountTotal")
        .or_else(|| status.get("peer_count_total"))
        .and_then(hex_or_decimal_to_u64);
    let peer_count_inbound = status
        .get("peerCountInbound")
        .or_else(|| status.get("peer_count_inbound"))
        .and_then(hex_or_decimal_to_u64);
    let peer_count_outbound = status
        .get("peerCountOutbound")
        .or_else(|| status.get("peer_count_outbound"))
        .and_then(hex_or_decimal_to_u64);

    let peers_str = format_startup_peers(peer_count_total, peer_count_inbound, peer_count_outbound);

    format!(
        "node :: {} | net={}{} | block={} | {}",
        client_version.unwrap_or_else(|| "unavailable".to_owned()),
        network_id.unwrap_or_else(|| "unavailable".to_owned()),
        chain_emoji(chain_id),
        head_number.unwrap_or_else(|| "unavailable".to_owned()),
        peers_str
    )
}

async fn print_startup_snapshot(
    rpc: &RpcClient,
    chain_id: Option<u64>,
    bera_admin_status: Option<&Value>,
) {
    if let Some(status) = bera_admin_status {
        println!("{}", format_beradmin_startup_line(status, chain_id));
    } else {
        let (version, block, peers, network) = tokio::join!(
            rpc.request_value("web3_clientVersion", None),
            rpc.request_value("eth_blockNumber", None),
            rpc.request_value("net_peerCount", None),
            rpc.request_value("net_version", None),
        );

        let version = version.ok().and_then(|v| as_string(&v));
        let block = block.ok().and_then(|v| hex_or_decimal_to_u64(&v).map(|n| n.to_string()));
        let peers = peers.ok().and_then(|v| hex_or_decimal_to_u64(&v).map(|n| n.to_string()));
        let network = network.ok().and_then(|v| as_string(&v));

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

fn effective_peer_total(total: Option<u64>, inbound: Option<u64>, outbound: Option<u64>) -> u64 {
    let in_count = inbound.unwrap_or(0);
    let out_count = outbound.unwrap_or(0);
    match total {
        Some(0) if in_count + out_count > 0 => in_count + out_count,
        Some(t) => t,
        None if in_count + out_count > 0 => in_count + out_count,
        None => 0,
    }
}

fn format_startup_peers(total: Option<u64>, inbound: Option<u64>, outbound: Option<u64>) -> String {
    if let (Some(in_count), Some(out_count)) = (inbound, outbound) {
        let peer_total = effective_peer_total(total, inbound, outbound);
        format!("peers={peer_total} (in={in_count} out={out_count})")
    } else {
        format!("peers={}", total.unwrap_or(0))
    }
}

fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn print_help() {
    println!("Commands:");
    println!(
        "  <method> [json_params]   (RPC call; dots become underscores, e.g. eth.blockNumber)"
    );
    println!("  TAB                      completion for RPC namespaces/methods");
    println!("  help | exit");
    println!("Queries (run against last RPC result):");
    println!("  .count | .len | .first | .last | .[0] | .[0].field | .map(.field)");
    println!("Destructive (calls admin.removePeer for each connected peer):");
    println!("  removeAllPeers | admin.removeAllPeers");
}

struct CompletionHelper {
    words: Vec<String>,
}

impl CompletionHelper {
    fn new(modules: &BTreeMap<String, String>, has_bera_admin: bool) -> CompletionHelper {
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
        for module in modules.keys() {
            words.push(format!("{module}."));
            words.push(format!("{module}_"));
            words.extend(super::rpc_completion::dot_completions_for_namespace(module));
        }
        if has_bera_admin {
            words.push("beradmin.".to_owned());
            words.push("beradmin_".to_owned());
            words.extend(super::rpc_completion::dot_completions_for_namespace("beradmin"));
        }
        words.sort();
        words.dedup();
        CompletionHelper { words }
    }
}

impl Completer for CompletionHelper {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let safe_pos = pos.min(line.len());
        let up_to_cursor = &line[..safe_pos];
        let start = up_to_cursor
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        let needle = &up_to_cursor[start..];
        self.words
            .iter()
            .filter(|word| word.starts_with(needle))
            .map(|word| Suggestion {
                value: word.clone(),
                span: Span::new(start, safe_pos),
                append_whitespace: false,
                ..Default::default()
            })
            .collect()
    }
}

pub fn history_file_path() -> PathBuf {
    default_datadir().join("bera-reth-console-history")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_includes_bera_admin_when_enabled() {
        let modules = BTreeMap::new();
        let helper = CompletionHelper::new(&modules, true);
        assert!(helper.words.iter().any(|w| w == "beradmin.detailedPeers"));
    }

    #[test]
    fn completion_excludes_bera_admin_when_disabled() {
        let modules = BTreeMap::new();
        let helper = CompletionHelper::new(&modules, false);
        assert!(!helper.words.iter().any(|w| w == "beradmin.detailedPeers"));
    }

    #[test]
    fn completion_matches_prefix() {
        let modules = BTreeMap::from([("eth".to_owned(), "1.0".to_owned())]);
        let mut helper = CompletionHelper::new(&modules, false);
        let hits = helper.complete("eth.getB", "eth.getB".len());
        assert!(hits.iter().any(|s| s.value == "eth.getBalance"));
    }

    #[test]
    fn completion_includes_eth_namespace_methods() {
        let modules = BTreeMap::from([("eth".to_owned(), "1.0".to_owned())]);
        let helper = CompletionHelper::new(&modules, false);
        assert!(helper.words.iter().any(|w| w == "eth.getLogs"));
        assert!(helper.words.iter().any(|w| w == "eth.getTransactionReceipt"));
    }

    #[test]
    fn parses_hex_or_decimal_numbers() {
        use serde_json::json;
        assert_eq!(hex_or_decimal_to_u64(&json!("0x10")), Some(16));
    }

    #[test]
    fn startup_peer_total_falls_back_to_inbound_plus_outbound() {
        assert_eq!(format_startup_peers(Some(0), Some(3), Some(2)), "peers=5 (in=3 out=2)");
        assert_eq!(format_startup_peers(None, Some(1), Some(4)), "peers=5 (in=1 out=4)");
    }

    #[test]
    fn beradmin_startup_line_from_camel_case_status() {
        use serde_json::json;
        let status = json!({
            "clientVersion": "bera-reth/test",
            "networkId": "80094",
            "headNumber": 100,
            "peerCountTotal": 0,
            "peerCountInbound": 3,
            "peerCountOutbound": 2,
        });
        let line = format_beradmin_startup_line(&status, Some(80_094));
        assert_eq!(
            line,
            "node :: bera-reth/test | net=80094 🐻 | block=100 | peers=5 (in=3 out=2)"
        );
    }

    #[test]
    fn beradmin_startup_line_accepts_snake_case_fields() {
        use serde_json::json;
        let status = json!({
            "client_version": "bera-reth/snake",
            "network_id": "1",
            "head_number": "0x64",
            "peer_count_total": 4,
            "peer_count_inbound": 2,
            "peer_count_outbound": 2,
        });
        let line = format_beradmin_startup_line(&status, None);
        assert_eq!(line, "node :: bera-reth/snake | net=1 | block=100 | peers=4 (in=2 out=2)");
    }
}
