use serde_json::Value as V;
pub(crate) fn content(e: &V) -> &str {
    e["content"]
        .as_str()
        .or(e["message"]["content"].as_str())
        .or(e["prompt"]["question"].as_str())
        .unwrap_or_default()
}
