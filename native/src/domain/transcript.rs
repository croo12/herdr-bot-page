use serde_json::Value;

#[derive(Clone, Copy)]
pub(crate) enum MessageSource<'a> {
    User,
    Bot(&'a str),
    Delegation(&'a str),
}

impl<'a> MessageSource<'a> {
    pub(crate) fn bot_id(self) -> Option<&'a str> {
        match self {
            Self::User => None,
            Self::Bot(id) | Self::Delegation(id) => Some(id),
        }
    }
}

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
