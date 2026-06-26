use serde_json::Value;

const DEFAULT_NATIVE_SYMBOL: &str = "ETH";

/// Berachain mainnet (80094) and bepolia testnet (80069) chain IDs.
pub(crate) const BERACHAIN_CHAIN_IDS: &[u64] = &[80_069, 80_094];

/// ~0.001 native tokens in wei — below this, unlikely to be a balance worth annotating.
const WEI_HEURISTIC_MIN: u128 = 1_000_000_000_000_000;
/// ~10 billion native tokens in wei — above this, not a plausible balance.
const WEI_HEURISTIC_MAX: u128 = 10_000_000_000_000_000_000_000_000_000;

pub(crate) fn hex_or_decimal_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::String(s) => {
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else {
                s.parse::<u64>().ok()
            }
        }
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// First `max_chars` Unicode scalars (never splits a codepoint).
fn chars_prefix(s: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    let mut end_byte = 0usize;
    let mut iter = s.char_indices();
    for _ in 0..max_chars {
        match iter.next() {
            Some((i, ch)) => end_byte = i + ch.len_utf8(),
            None => return s,
        }
    }
    &s[..end_byte]
}

/// `prefix..suffix` with lengths in characters; returns `s` unchanged if it fits.
fn ellipsis_char_ends(s: &str, prefix_chars: usize, suffix_chars: usize) -> String {
    let n = s.chars().count();
    if n <= prefix_chars + suffix_chars {
        return s.to_string();
    }
    let prefix: String = s.chars().take(prefix_chars).collect();
    let suffix: String = s.chars().skip(n - suffix_chars).collect();
    format!("{prefix}..{suffix}")
}

pub fn print_value_for_chain(value: &Value, chain_id: Option<u64>) {
    print_value_with_symbol(value, native_symbol_for_chain_id(chain_id), false);
}

pub fn print_value_for_chain_raw(value: &Value, chain_id: Option<u64>, raw: bool) {
    if raw {
        println!("{}", pretty(value));
    } else {
        print_value_for_chain(value, chain_id);
    }
}

pub fn native_symbol_for_chain_id(chain_id: Option<u64>) -> &'static str {
    match chain_id {
        Some(id) if BERACHAIN_CHAIN_IDS.contains(&id) => "BERA",
        _ => DEFAULT_NATIVE_SYMBOL,
    }
}

fn print_value_with_symbol(value: &Value, native_symbol: &str, raw: bool) {
    if raw {
        println!("{}", pretty(value));
        return;
    }

    if let Some(table) = try_format_detailed_peers(value) {
        println!("{}", table);
        return;
    }
    if let Some(status) = try_format_node_status(value) {
        println!("{}", status);
        return;
    }
    if let Some(table) = try_format_peer_scores(value) {
        println!("{}", table);
        return;
    }
    if let Some(table) = try_format_banned_subnets(value) {
        println!("{}", table);
        return;
    }

    println!("{}", pretty(value));

    let annotations = collect_annotations_with_symbol(value, native_symbol);
    if !annotations.is_empty() {
        println!("-- interpreted values --");
        for note in annotations {
            println!("{note}");
        }
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn collect_annotations_with_symbol(value: &Value, native_symbol: &str) -> Vec<String> {
    let mut out = Vec::new();
    walk("$", value, native_symbol, &mut out);
    out
}

fn walk(path: &str, value: &Value, native_symbol: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                walk(&format!("{path}.{k}"), v, native_symbol, out);
            }
        }
        Value::Array(items) => {
            for (idx, v) in items.iter().enumerate() {
                walk(&format!("{path}[{idx}]"), v, native_symbol, out);
            }
        }
        Value::String(s) => {
            if let Some(dec) = small_hex_to_dec(s) {
                out.push(format!("{path}: {s} -> {dec}"));
                if looks_like_wei(dec) {
                    out.push(format!("{path}: {dec} wei -> {} {native_symbol}", format_eth(dec)));
                }
            }
            if let Some(wei) = decimal_like_wei(s) {
                out.push(format!("{path}: {wei} wei -> {} {native_symbol}", format_eth(wei)));
            }
        }
        Value::Number(n) => {
            if let Some(wei) = n.as_u64().map(u128::from) &&
                looks_like_wei(wei)
            {
                out.push(format!("{path}: {wei} wei -> {} {native_symbol}", format_eth(wei)));
            }
        }
        _ => {}
    }
}

fn small_hex_to_dec(input: &str) -> Option<u128> {
    if !(input.starts_with("0x") || input.starts_with("0X")) {
        return None;
    }
    let hex = &input[2..];
    if hex.is_empty() || hex.len() > 32 {
        return None;
    }
    u128::from_str_radix(hex, 16).ok()
}

fn decimal_like_wei(input: &str) -> Option<u128> {
    if input.is_empty() || !input.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let value = input.parse::<u128>().ok()?;
    if looks_like_wei(value) { Some(value) } else { None }
}

fn looks_like_wei(value: u128) -> bool {
    (WEI_HEURISTIC_MIN..=WEI_HEURISTIC_MAX).contains(&value)
}

fn format_eth(wei: u128) -> String {
    const WEI_PER_ETH: u128 = 1_000_000_000_000_000_000;
    let whole = wei / WEI_PER_ETH;
    let frac = wei % WEI_PER_ETH;
    if frac == 0 {
        return whole.to_string();
    }
    let mut frac_str = format!("{frac:018}");
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{whole}.{frac_str}")
}

fn try_format_detailed_peers(value: &Value) -> Option<String> {
    let peers = value.as_array()?;
    if peers.is_empty() {
        return Some("-- no peers connected --".to_string());
    }

    let first = peers.first()?;
    if !first.is_object() {
        return None;
    }

    let obj = first.as_object()?;
    if !obj.contains_key("peerId") && !obj.contains_key("peer_id") {
        return None;
    }

    let mut lines = vec![];
    let header = format!(
        "{:<18} {:<18} {:<4} {:<4} {:<6} {:<14} {:<8} {:<10}",
        "PEER", "ADDR", "DIR", "REP", "BLOCK", "CLIENT", "STATE", "PoG"
    );
    lines.push(header);

    for peer in peers {
        if let Some(peer_obj) = peer.as_object() {
            let peer_id = peer_obj
                .get("peerId")
                .or_else(|| peer_obj.get("peer_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let addr = peer_obj
                .get("remoteAddr")
                .or_else(|| peer_obj.get("remote_addr"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let direction = peer_obj
                .get("direction")
                .and_then(|v| v.as_str())
                .map(|s| chars_prefix(s, 3))
                .unwrap_or("-");
            let reputation = peer_obj
                .get("reputation")
                .and_then(|v| v.as_i64())
                .map(|r| r.to_string())
                .unwrap_or_else(|| "?".to_string());
            let block = peer_obj
                .get("latestBlock")
                .or_else(|| peer_obj.get("latest_block"))
                .and_then(|v| v.as_u64())
                .map(|b| b.to_string())
                .unwrap_or_else(|| "?".to_string());
            let client = peer_obj
                .get("clientVersion")
                .or_else(|| peer_obj.get("client_version"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let client_short = if client.chars().count() > 14 {
                ellipsis_char_ends(client, 8, 3)
            } else {
                client.to_string()
            };
            let state = peer_obj
                .get("connectionState")
                .or_else(|| peer_obj.get("connection_state"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");

            let pog_str = if let Some(pog) = peer_obj.get("pog") {
                if pog.is_null() {
                    "-".to_string()
                } else if let Some(pog_obj) = pog.as_object() {
                    let failures = pog_obj
                        .get("failureCount")
                        .or_else(|| pog_obj.get("failure_count"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    format!("{}", failures)
                } else {
                    "-".to_string()
                }
            } else {
                "-".to_string()
            };

            let peer_short = if peer_id.chars().count() > 12 {
                ellipsis_char_ends(peer_id, 6, 4)
            } else {
                peer_id.to_string()
            };

            let line = format!(
                "{:<18} {:<18} {:<4} {:<4} {:<6} {:<14} {:<8} {:<10}",
                peer_short, addr, direction, reputation, block, client_short, state, pog_str
            );
            lines.push(line);
        }
    }

    Some(lines.join("\n"))
}

fn try_format_node_status(value: &Value) -> Option<String> {
    let obj = value.as_object()?;

    if !obj.contains_key("chainId") &&
        !obj.contains_key("chain_id") &&
        !obj.contains_key("genesisHash") &&
        !obj.contains_key("genesis_hash") &&
        !obj.contains_key("headNumber") &&
        !obj.contains_key("head_number")
    {
        return None;
    }

    let chain = obj
        .get("chainId")
        .or_else(|| obj.get("chain_id"))
        .and_then(hex_or_decimal_to_u64)
        .unwrap_or(0);
    let network = obj
        .get("networkId")
        .or_else(|| obj.get("network_id"))
        .and_then(|v| hex_or_decimal_to_u64(v).or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0);
    let genesis = obj
        .get("genesisHash")
        .or_else(|| obj.get("genesis_hash"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let genesis_short = if genesis.chars().count() > 12 {
        ellipsis_char_ends(genesis, 6, 4)
    } else {
        genesis.to_string()
    };
    let head = obj
        .get("headNumber")
        .or_else(|| obj.get("head_number"))
        .and_then(hex_or_decimal_to_u64)
        .unwrap_or(0);
    let head_hash = obj
        .get("headHash")
        .or_else(|| obj.get("head_hash"))
        .or_else(|| obj.get("head"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let head_hash_short = if head_hash.chars().count() > 12 {
        ellipsis_char_ends(head_hash, 6, 4)
    } else {
        head_hash.to_string()
    };
    let syncing = obj.get("syncing").and_then(|v| v.as_bool()).unwrap_or(false);
    let peers_in = obj
        .get("peerCountInbound")
        .or_else(|| obj.get("peer_count_inbound"))
        .and_then(hex_or_decimal_to_u64)
        .unwrap_or(0);
    let peers_out = obj
        .get("peerCountOutbound")
        .or_else(|| obj.get("peer_count_outbound"))
        .and_then(hex_or_decimal_to_u64)
        .unwrap_or(0);
    let peers_total = {
        let total = obj
            .get("peerCountTotal")
            .or_else(|| obj.get("peer_count_total"))
            .and_then(hex_or_decimal_to_u64)
            .unwrap_or(0);
        if total == 0 && peers_in + peers_out > 0 {
            peers_in + peers_out
        } else {
            total
        }
    };
    let client = obj
        .get("clientVersion")
        .or_else(|| obj.get("client_version"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let output = format!(
        "chain={}  genesis={}  fork=unknown\nhead={} ({})  syncing={}\npeers={} (in={} out={})  client={}  net={}",
        chain,
        genesis_short,
        head,
        head_hash_short,
        syncing,
        peers_total,
        peers_in,
        peers_out,
        client,
        network
    );

    Some(output)
}

fn try_format_peer_scores(value: &Value) -> Option<String> {
    let scores = value.as_array()?;
    if scores.is_empty() {
        return Some("-- no peers scored --".to_string());
    }

    let first = scores.first()?;
    if !first.is_object() {
        return None;
    }

    let obj = first.as_object()?;
    // Detect if this looks like peer scores: should have fields like peerId, threatScore, node,
    // policies
    if !obj.contains_key("peerId") && !obj.contains_key("peer_id") {
        return None;
    }
    if !obj.contains_key("threatScore") && !obj.contains_key("threat_score") {
        return None;
    }

    let mut lines = vec![];
    let header =
        format!("{:<18} {:<6} {:<10} {:<14} {:<8}", "PEER", "THREAT", "NODE", "REASON", "POLICIES");
    lines.push(header);

    for score in scores {
        if let Some(score_obj) = score.as_object() {
            let peer_id = score_obj
                .get("peerId")
                .or_else(|| score_obj.get("peer_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let threat = score_obj
                .get("threatScore")
                .or_else(|| score_obj.get("threat_score"))
                .and_then(|v| v.as_u64())
                .map(|t| t.to_string())
                .unwrap_or_else(|| "?".to_string());
            let node = score_obj.get("node").and_then(|v| v.as_str()).unwrap_or("?");
            let reason = score_obj.get("reason").and_then(|v| v.as_str()).unwrap_or("-");

            let policies = score_obj
                .get("policies")
                .and_then(|v| v.as_array())
                .map(|p| p.len().to_string())
                .unwrap_or_else(|| "0".to_string());

            let peer_short = if peer_id.chars().count() > 12 {
                ellipsis_char_ends(peer_id, 6, 4)
            } else {
                peer_id.to_string()
            };
            let reason_short = if reason.chars().count() > 14 {
                ellipsis_char_ends(reason, 8, 3)
            } else {
                reason.to_string()
            };

            let line = format!(
                "{:<18} {:<6} {:<10} {:<14} {:<8}",
                peer_short, threat, node, reason_short, policies
            );
            lines.push(line);
        }
    }

    Some(lines.join("\n"))
}

fn try_format_banned_subnets(value: &Value) -> Option<String> {
    let subnets = value.as_array()?;
    if subnets.is_empty() {
        return Some("-- no subnets banned --".to_string());
    }

    let first = subnets.first()?;
    if !first.is_object() {
        return None;
    }

    let obj = first.as_object()?;
    // Detect if this looks like banned subnets: should have fields like subnet, reason, peers,
    // nodes
    if !obj.contains_key("subnet") && !obj.contains_key("cidr") {
        return None;
    }

    let mut lines = vec![];
    let header = format!("{:<20} {:<14} {:<8} {:<20}", "SUBNET", "REASON", "PEERS", "NODES");
    lines.push(header);

    for subnet in subnets {
        if let Some(subnet_obj) = subnet.as_object() {
            let cidr = subnet_obj
                .get("subnet")
                .or_else(|| subnet_obj.get("cidr"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let reason = subnet_obj.get("reason").and_then(|v| v.as_str()).unwrap_or("-");
            let peer_count = subnet_obj
                .get("peerCount")
                .or_else(|| subnet_obj.get("peer_count"))
                .and_then(|v| v.as_u64())
                .map(|p| p.to_string())
                .unwrap_or_else(|| "?".to_string());
            let nodes = subnet_obj
                .get("nodes")
                .and_then(|v| v.as_array())
                .map(|n| n.len().to_string())
                .unwrap_or_else(|| "?".to_string());

            let reason_short = if reason.chars().count() > 14 {
                ellipsis_char_ends(reason, 8, 3)
            } else {
                reason.to_string()
            };

            let line = format!("{:<20} {:<14} {:<8} {:<20}", cidr, reason_short, peer_count, nodes);
            lines.push(line);
        }
    }

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_small_hex() {
        assert_eq!(small_hex_to_dec("0x2a"), Some(42));
        assert_eq!(small_hex_to_dec("0xffffffffffffffff"), Some(u64::MAX as u128));
        assert_eq!(small_hex_to_dec("0x10000000000000000"), Some(18446744073709551616));
        assert_eq!(small_hex_to_dec("0x100000000000000000000000000000000"), None);
    }

    #[test]
    fn formats_eth() {
        assert_eq!(format_eth(1_000_000_000_000_000_000), "1");
        assert_eq!(format_eth(1_500_000_000_000_000_000), "1.5");
    }

    #[test]
    fn annotates_wei_and_hex() {
        let value = serde_json::json!({
            "gasUsed": "0x5208",
            "amount": "1000000000000000000"
        });
        let notes = collect_annotations_with_symbol(&value, DEFAULT_NATIVE_SYMBOL);
        assert!(notes.iter().any(|n| n.contains("0x5208 -> 21000")));
        assert!(notes.iter().any(|n| n.contains("1 ETH")));
    }

    #[test]
    fn annotates_hex_wei_as_eth() {
        let value = serde_json::json!("0x0de0b6b3a7640000");
        let notes = collect_annotations_with_symbol(&value, DEFAULT_NATIVE_SYMBOL);
        assert!(notes.iter().any(|n| n.contains("1000000000000000000")));
        assert!(notes.iter().any(|n| n.contains("1 ETH")));
    }

    #[test]
    fn detects_bera_native_symbol() {
        assert_eq!(native_symbol_for_chain_id(Some(80_069)), "BERA");
        assert_eq!(native_symbol_for_chain_id(Some(80_094)), "BERA");
        assert_eq!(native_symbol_for_chain_id(Some(1)), "ETH");
        assert_eq!(native_symbol_for_chain_id(None), "ETH");
    }

    #[test]
    fn annotates_native_symbol_for_chain() {
        let value = serde_json::json!("1000000000000000000");
        let notes = collect_annotations_with_symbol(&value, "BERA");
        assert!(notes.iter().any(|n| n.contains("1 BERA")));
    }

    #[test]
    fn formats_peer_scores_table() {
        let scores = serde_json::json!([
            {
                "peerId": "0xabcdef1234567890abcdef1234567890abcdef12",
                "threatScore": 150,
                "node": "node-1",
                "reason": "stale_head",
                "policies": ["stale_head", "subnet_concentration"]
            }
        ]);
        let formatted = try_format_peer_scores(&scores);
        assert!(formatted.is_some());
        let formatted_str = formatted.unwrap();
        assert!(formatted_str.contains("PEER"));
        assert!(formatted_str.contains("THREAT"));
        assert!(formatted_str.contains("0xabcd..ef12"));
        assert!(formatted_str.contains("150"));
    }

    #[test]
    fn formats_banned_subnets_table() {
        let subnets = serde_json::json!([
            {
                "subnet": "192.168.1.0/24",
                "reason": "subnet_concentration",
                "peerCount": 5,
                "nodes": ["node-1", "node-2"]
            }
        ]);
        let formatted = try_format_banned_subnets(&subnets);
        assert!(formatted.is_some());
        let formatted_str = formatted.unwrap();
        assert!(formatted_str.contains("SUBNET"));
        assert!(formatted_str.contains("PEERS"));
        assert!(formatted_str.contains("192.168.1.0/24"));
        assert!(formatted_str.contains("5"));
    }

    #[test]
    fn handles_empty_peer_scores_gracefully() {
        let empty_scores = serde_json::json!([]);
        let formatted = try_format_peer_scores(&empty_scores);
        assert_eq!(formatted, Some("-- no peers scored --".to_string()));
    }

    #[test]
    fn handles_empty_banned_subnets_gracefully() {
        let empty_subnets = serde_json::json!([]);
        let formatted = try_format_banned_subnets(&empty_subnets);
        assert_eq!(formatted, Some("-- no subnets banned --".to_string()));
    }

    #[test]
    fn chars_prefix_never_splits_codepoints() {
        assert_eq!(chars_prefix("μμμbound", 3), "μμμ");
        assert_eq!(chars_prefix("μμμbound", 0), "");
    }

    #[test]
    fn ellipsis_char_ends_counts_chars_not_bytes() {
        let out = ellipsis_char_ends("abcdefghijklmnopqrs", 6, 4);
        assert_eq!(out, "abcdef..pqrs");
    }

    #[test]
    fn node_status_keeps_chain_and_network_distinct() {
        let status = serde_json::json!({
            "chainId": 80094,
            "networkId": 80094,
            "genesisHash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "headNumber": 100,
            "headHash": "0x1234",
            "syncing": false,
            "peerCountTotal": 0,
            "peerCountInbound": 2,
            "peerCountOutbound": 3,
            "clientVersion": "bera-reth/test"
        });
        let formatted = try_format_node_status(&status).expect("formatted");
        assert!(formatted.contains("chain=80094"));
        assert!(formatted.contains("net=80094"));
        assert!(formatted.contains("peers=5 (in=2 out=3)"));
    }

    #[test]
    fn node_status_uses_network_id_not_chain_for_net_field() {
        let status = serde_json::json!({
            "chainId": 80094,
            "networkId": 1,
            "genesisHash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "headNumber": 1,
            "headHash": "0x1234",
            "syncing": false,
            "peerCountTotal": 4,
            "peerCountInbound": 2,
            "peerCountOutbound": 2,
            "clientVersion": "bera-reth/test"
        });
        let formatted = try_format_node_status(&status).expect("formatted");
        assert!(formatted.contains("chain=80094"));
        assert!(formatted.contains("net=1"));
    }
}
