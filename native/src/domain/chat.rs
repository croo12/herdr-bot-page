use crate::domain::{now, patch, string_field};
use serde_json::{json, Value};
pub(crate) fn new_chat(id: &str, kind: &str, params: &Value) -> Value {
    let timestamp = now();
    let mut chat = json!({"id":id,"kind":kind,"name":string_field(params,"name"),"description":"","title":"","avatarShape":null,"avatarColor":null,
        "createdAt":timestamp,"updatedAt":timestamp,"isRunning":false,"currentActivity":null,"lastMessagePreview":null,"lastMessageId":null,
        "hasUnread":false,"unreadCount":0,"lastViewedAt":0,"lastActivityAt":timestamp,"lastReadSeq":0,"lastIncomingSeq":0,"lastMessageAt":0,
        "awaitingUserResponse":null,"notifyOnUpdatesEnabled":true,"isHiddenFromSidebar":false,"isGroup":kind=="room","memberIds":[],"bot":null});
    patch(
        &mut chat,
        params,
        &[
            "description",
            "title",
            "avatarShape",
            "avatarColor",
            "memberIds",
        ],
    );
    chat
}

pub(crate) fn record_latest_entry(chat: &mut Value, entry: &Value) {
    chat["updatedAt"] = entry["timestampMs"].clone();
    chat["lastActivityAt"] = entry["timestampMs"].clone();
    chat["lastMessageAt"] = entry["timestampMs"].clone();
    chat["lastMessagePreview"] = super::content(entry)
        .chars()
        .take(180)
        .collect::<String>()
        .into();
    chat["lastMessageId"] = entry["id"].clone();
    if entry["kind"] != "message" {
        chat["lastIncomingSeq"] = entry["seq"].clone();
        chat["hasUnread"] = true.into();
        chat["unreadCount"] = (chat["unreadCount"].as_i64().unwrap_or(0) + 1).into();
    }
}
