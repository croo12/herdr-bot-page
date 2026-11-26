use serde_json::Value;
pub(crate) fn content(entry: &Value) -> &str {
    entry["content"]
        .as_str()
        .or(entry["message"]["content"].as_str())
        .or(entry["prompt"]["question"].as_str())
        .unwrap_or_default()
}
