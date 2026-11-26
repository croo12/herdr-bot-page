use serde_json::Value;
pub(crate) fn content(entry: &Value) -> &str {
    entry["content"]
        .as_str()
        .or(entry["message"]["content"].as_str())
        .or(entry["prompt"]["question"].as_str())
        .unwrap_or_default()
}

pub(crate) fn permission_reply(entry: &Value, review: &Value) -> Value {
    serde_json::json!({"kind":"send-message","author":{"id":review["reviewerId"],"name":review["name"]},
        "replyTo":entry["id"],"permissionDecision":review["decision"],
        "message":{"type":"text","content":review["reason"]}})
}
