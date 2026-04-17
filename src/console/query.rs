use eyre::{Result, bail, eyre};
use serde_json::{Number, Value};

#[derive(Debug, Clone)]
enum Segment {
    Field(String),
    Index(usize),
}

pub fn apply_query(expr: &str, input: &Value) -> Result<Value> {
    let expr = expr.trim();
    if !expr.starts_with('.') {
        bail!("query must start with '.'");
    }

    if let Some((inner, rest)) = parse_map(expr)? {
        let items = input
            .as_array()
            .ok_or_else(|| eyre!(".map(...) requires last result to be an array"))?;
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(apply_query(inner, item)?);
        }
        let mapped = Value::Array(out);
        if rest.is_empty() {
            return Ok(mapped);
        }
        return apply_query(rest, &mapped);
    }

    if expr == ".count" || expr == ".len" {
        return count_value(input);
    }
    if expr == ".first" {
        return first_value(input);
    }
    if expr == ".last" {
        return last_value(input);
    }

    let segments = parse_segments(expr)?;
    let mut current = input;
    for segment in segments {
        current = match segment {
            Segment::Field(name) => {
                current.get(&name).ok_or_else(|| eyre!("field {name:?} not found"))?
            }
            Segment::Index(i) => current.get(i).ok_or_else(|| eyre!("index {i} out of range"))?,
        };
    }
    Ok(current.clone())
}

fn parse_map(expr: &str) -> Result<Option<(&str, &str)>> {
    if !expr.starts_with(".map(") {
        return Ok(None);
    }
    let mut depth = 1usize;
    let mut close = None;
    for (i, b) in expr.bytes().enumerate().skip(5) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.ok_or_else(|| eyre!("invalid .map(...) expression"))?;
    let inner = &expr[5..close];
    if !inner.starts_with('.') {
        bail!("map selector must start with '.'");
    }
    let rest = &expr[close + 1..];
    Ok(Some((inner, rest)))
}

fn parse_segments(expr: &str) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'.' {
            bail!("invalid query syntax near {}", &expr[i..]);
        }
        i += 1;
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'[' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b']' {
                i += 1;
            }
            if i >= bytes.len() {
                bail!("unterminated index in query");
            }
            let idx: usize = expr[start..i].parse()?;
            out.push(Segment::Index(idx));
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'.' || c == b'[' {
                break;
            }
            i += 1;
        }
        let name = expr[start..i].trim();
        if name.is_empty() {
            bail!("empty field in query");
        }
        out.push(Segment::Field(name.to_owned()));
    }
    Ok(out)
}

fn count_value(value: &Value) -> Result<Value> {
    match value {
        Value::Array(items) => Ok(Value::Number(Number::from(items.len() as u64))),
        Value::Object(fields) => Ok(Value::Number(Number::from(fields.len() as u64))),
        _ => Err(eyre!(".count/.len only apply to arrays and objects")),
    }
}

fn first_value(value: &Value) -> Result<Value> {
    let items = value.as_array().ok_or_else(|| eyre!(".first only applies to arrays"))?;
    Ok(items.first().cloned().unwrap_or(Value::Null))
}

fn last_value(value: &Value) -> Result<Value> {
    let items = value.as_array().ok_or_else(|| eyre!(".last only applies to arrays"))?;
    Ok(items.last().cloned().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn applies_count() {
        let value = json!([1, 2, 3]);
        let out = apply_query(".count", &value).unwrap();
        assert_eq!(out, json!(3));
    }

    #[test]
    fn applies_len() {
        let value = json!({"a": 1, "b": 2});
        let out = apply_query(".len", &value).unwrap();
        assert_eq!(out, json!(2));
    }

    #[test]
    fn applies_index_and_field() {
        let value = json!([{ "a": 1 }, { "a": 2 }]);
        let out = apply_query(".[1].a", &value).unwrap();
        assert_eq!(out, json!(2));
    }

    #[test]
    fn applies_map() {
        let value = json!([{ "a": 1 }, { "a": 2 }]);
        let out = apply_query(".map(.a)", &value).unwrap();
        assert_eq!(out, json!([1, 2]));
    }

    #[test]
    fn applies_map_then_count() {
        let value = json!([{ "a": 1 }, { "a": 2 }, { "a": 3 }]);
        let out = apply_query(".map(.a).count", &value).unwrap();
        assert_eq!(out, json!(3));
    }

    #[test]
    fn first_and_last_on_empty_array_return_null() {
        let value = json!([]);
        assert_eq!(apply_query(".first", &value).unwrap(), Value::Null);
        assert_eq!(apply_query(".last", &value).unwrap(), Value::Null);
    }

    #[test]
    fn map_requires_array_input() {
        let value = json!({"a": 1});
        let err = apply_query(".map(.a)", &value).unwrap_err();
        assert!(err.to_string().contains("requires last result to be an array"));
    }

    #[test]
    fn count_rejects_scalars() {
        let value = json!("abc");
        let err = apply_query(".count", &value).unwrap_err();
        assert!(err.to_string().contains("only apply to arrays and objects"));
    }
}
