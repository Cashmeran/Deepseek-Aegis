//! JSON schema helpers — truncation repair, schema validation

/// Attempt to repair truncated JSON by balancing braces/brackets
pub fn repair_truncated_json(json_str: &str) -> Option<String> {
    let s = json_str.trim();
    if s.is_empty() { return None; }

    let open_braces = s.matches('{').count() as isize - s.matches('}').count() as isize;
    let open_brackets = s.matches('[').count() as isize - s.matches(']').count() as isize;
    let open_quotes = s.matches('"').count() % 2 != 0;

    let mut fixed = s.to_string();
    if open_quotes { fixed.push('"'); }
    for _ in 0..open_braces.max(0) { fixed.push('}'); }
    for _ in 0..open_brackets.max(0) { fixed.push(']'); }

    serde_json::from_str::<serde_json::Value>(&fixed).ok()?;
    Some(fixed)
}

/// Extract a JSON object from text (first {...} found)
pub fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 { return Some(&text[start..start + i + 1]); }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repair_missing_brace() {
        let fixed = repair_truncated_json(r#"{"name":"test""#);
        assert!(fixed.is_some());
        assert!(serde_json::from_str::<serde_json::Value>(&fixed.unwrap()).is_ok());
    }

    #[test]
    fn test_extract_json_object() {
        let text = "prefix {\"key\": \"val\"} suffix";
        let obj = extract_json_object(text).unwrap();
        assert_eq!(obj, r#"{"key": "val"}"#);
    }

    #[test]
    fn test_repair_valid_stays_same() {
        let fixed = repair_truncated_json(r#"{"a":1}"#);
        assert_eq!(fixed.unwrap(), r#"{"a":1}"#);
    }
}
