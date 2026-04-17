use eyre::{Result, eyre};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum InputCommand {
    Empty,
    Help,
    Exit,
    Query(String),
    Rpc { method: String, params: Option<Value> },
    RpcWithQuery { method: String, params: Option<Value>, query: String },
    /// Single token treated as a method name (`eth.blockNumber` → `eth_blockNumber`).
    MethodToken(String),
}

pub fn parse_input(line: &str) -> Result<InputCommand> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(InputCommand::Empty);
    }
    if line == "help" || line == "?" {
        return Ok(InputCommand::Help);
    }
    if line == "exit" || line == "quit" {
        return Ok(InputCommand::Exit);
    }
    if line.starts_with('.') {
        return Ok(InputCommand::Query(line.to_owned()));
    }
    if let Some((rpc_part, query)) = split_rpc_query_tail(line) {
        let (method, params_raw) = split_method_and_params(rpc_part)?;
        let params = parse_params(params_raw)?;
        return Ok(InputCommand::RpcWithQuery { method, params, query });
    }
    if looks_like_implicit_rpc(line) {
        return parse_rpc(line);
    }
    Ok(InputCommand::MethodToken(line.to_owned()))
}

fn split_rpc_query_tail(line: &str) -> Option<(&str, String)> {
    if line.contains(char::is_whitespace) || line.contains('(') {
        return None;
    }

    let query_start = if let Some(p1) = line.find('.') {
        let before_dot = &line[..p1];
        if before_dot.contains('_') {
            // Underscore-style method (e.g. admin_peers): first '.' or '[' is query start.
            let first_bracket = line.find('[');
            match first_bracket {
                Some(b) if b < p1 => Some(b),
                _ => Some(p1),
            }
        } else {
            // Dot-style method (e.g. admin.peers): consume two segments, remainder is query.
            let after_p1 = p1 + 1;
            let seg2_len = line[after_p1..]
                .find(|c: char| ['.', '['].contains(&c))
                .unwrap_or(line.len() - after_p1);
            let seg2_end = after_p1 + seg2_len;
            if seg2_end < line.len() { Some(seg2_end) } else { None }
        }
    } else {
        line.find('[')
    }?;

    let rpc_part = &line[..query_start];
    let raw_tail = &line[query_start..];
    if raw_tail.is_empty() {
        return None;
    }
    let query =
        if raw_tail.starts_with('[') { format!(".{raw_tail}") } else { raw_tail.to_owned() };
    Some((rpc_part, query))
}

fn parse_rpc(rest: &str) -> Result<InputCommand> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(eyre!("usage: <method> [json_params]"));
    }
    let (method, params_raw) = split_method_and_params(rest)?;
    let parsed_params = parse_params(params_raw)?;
    Ok(InputCommand::Rpc { method, params: parsed_params })
}

fn looks_like_implicit_rpc(line: &str) -> bool {
    // Accept direct method calls like:
    // - eth_getBalance ["0x...", "latest"]
    // - eth.getBalance ["0x...", "latest"]
    // Single-token `eth.blockNumber`-style input is handled as MethodToken.
    line.contains(char::is_whitespace) ||
        (line.contains('(') && line.ends_with(')')) ||
        (line.contains('_') && !line.contains(' '))
}

fn split_method_and_params(input: &str) -> Result<(String, Option<&str>)> {
    let input = input.trim();
    if let Some(paren_start) = input.find('(') {
        let method = input[..paren_start].trim();
        if method.is_empty() {
            return Err(eyre!("missing method name"));
        }
        if !input.ends_with(')') {
            return Err(eyre!("unbalanced parentheses in method call"));
        }
        let inner = &input[paren_start + 1..input.len() - 1];
        let params = if inner.trim().is_empty() { None } else { Some(inner.trim()) };
        return Ok((method.to_owned(), params));
    }

    let mut split = input.splitn(2, char::is_whitespace);
    let method = split.next().unwrap_or_default().trim();
    if method.is_empty() {
        return Err(eyre!("missing method name"));
    }
    let params = split.next().map(str::trim).filter(|s| !s.is_empty());
    Ok((method.to_owned(), params))
}

fn parse_params(params_raw: Option<&str>) -> Result<Option<Value>> {
    let Some(raw) = params_raw else {
        return Ok(None);
    };
    let mut s = raw.trim();
    while s.starts_with('(') && s.ends_with(')') && s.len() >= 2 {
        s = s[1..s.len() - 1].trim();
    }
    if s.is_empty() {
        return Ok(None);
    }

    // If wrapped as a JSON array, use it directly as RPC positional params.
    if s.starts_with('[') && s.ends_with(']') {
        return Ok(Some(serde_json::from_str::<Value>(s)?));
    }
    // If wrapped as a JSON object, pass as named params.
    if s.starts_with('{') && s.ends_with('}') {
        return Ok(Some(serde_json::from_str::<Value>(s)?));
    }

    // Function-style "a, b, c" => parse as positional params.
    let array_form = format!("[{s}]");
    if let Ok(v) = serde_json::from_str::<Value>(&array_form) {
        return Ok(Some(v));
    }

    // Single scalar JSON value fallback.
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        return Ok(Some(Value::Array(vec![v])));
    }

    Err(eyre!("unable to parse params; use JSON values, e.g. [\"0x...\", \"latest\"]"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_rpc_with_params() {
        let cmd = parse_input(r#"eth_getBlockByNumber ["latest", false]"#).unwrap();
        assert_eq!(
            cmd,
            InputCommand::Rpc {
                method: "eth_getBlockByNumber".to_owned(),
                params: Some(json!(["latest", false])),
            }
        );
    }

    #[test]
    fn parses_query() {
        let cmd = parse_input(".count").unwrap();
        assert_eq!(cmd, InputCommand::Query(".count".to_owned()));
    }

    #[test]
    fn parses_implicit_rpc_with_params() {
        let cmd = parse_input(r#"eth.getBalance ["0xabc", "latest"]"#).unwrap();
        assert_eq!(
            cmd,
            InputCommand::Rpc {
                method: "eth.getBalance".to_owned(),
                params: Some(json!(["0xabc", "latest"])),
            }
        );
    }

    #[test]
    fn parses_parenthesized_call_with_array() {
        let cmd = parse_input(r#"eth.getBalance(["0xabc", "latest"])"#).unwrap();
        assert_eq!(
            cmd,
            InputCommand::Rpc {
                method: "eth.getBalance".to_owned(),
                params: Some(json!(["0xabc", "latest"])),
            }
        );
    }

    #[test]
    fn parses_parenthesized_call_without_array() {
        let cmd = parse_input(r#"eth.getBalance("0xabc", "latest")"#).unwrap();
        assert_eq!(
            cmd,
            InputCommand::Rpc {
                method: "eth.getBalance".to_owned(),
                params: Some(json!(["0xabc", "latest"])),
            }
        );
    }

    #[test]
    fn parses_empty_as_empty_command() {
        assert_eq!(parse_input("   ").unwrap(), InputCommand::Empty);
    }

    #[test]
    fn keeps_dot_method_token_when_no_params() {
        assert_eq!(
            parse_input("eth.blockNumber").unwrap(),
            InputCommand::MethodToken("eth.blockNumber".to_owned())
        );
    }

    #[test]
    fn errors_on_unbalanced_parentheses() {
        let err = parse_input(r#"eth.getBalance("0xabc", "latest""#).unwrap_err();
        assert!(err.to_string().contains("unbalanced parentheses"));
    }

    #[test]
    fn errors_on_invalid_params() {
        let err = parse_input(r#"eth_getBalance [broken"#).unwrap_err();
        assert!(err.to_string().contains("unable to parse params"));
    }

    #[test]
    fn chained_dot_count() {
        let cmd = parse_input("admin.peers.count").unwrap();
        assert_eq!(
            cmd,
            InputCommand::RpcWithQuery {
                method: "admin.peers".to_owned(),
                params: None,
                query: ".count".to_owned(),
            }
        );
    }

    #[test]
    fn chained_bracket_index_and_field() {
        let cmd = parse_input("admin.peers[0].caps").unwrap();
        assert_eq!(
            cmd,
            InputCommand::RpcWithQuery {
                method: "admin.peers".to_owned(),
                params: None,
                query: ".[0].caps".to_owned(),
            }
        );
    }

    #[test]
    fn chained_bracket_only() {
        let cmd = parse_input("admin.peers[0]").unwrap();
        assert_eq!(
            cmd,
            InputCommand::RpcWithQuery {
                method: "admin.peers".to_owned(),
                params: None,
                query: ".[0]".to_owned(),
            }
        );
    }

    #[test]
    fn chained_underscore_style() {
        let cmd = parse_input("admin_peers.count").unwrap();
        assert_eq!(
            cmd,
            InputCommand::RpcWithQuery {
                method: "admin_peers".to_owned(),
                params: None,
                query: ".count".to_owned(),
            }
        );
    }

    #[test]
    fn no_chain_on_plain_method_token() {
        assert_eq!(
            parse_input("admin.peers").unwrap(),
            InputCommand::MethodToken("admin.peers".to_owned())
        );
    }
}
