use crate::domain::{now, patch, s};
use serde_json::{json, Value as V};
pub(crate) fn new_chat(id: &str, kind: &str, p: &V) -> V {
    let t = now();
    let mut v = json!({"id":id,"kind":kind,"name":s(p,"name"),"description":"","title":"","avatarShape":null,"avatarColor":null,
        "createdAt":t,"updatedAt":t,"isRunning":false,"currentActivity":null,"lastMessagePreview":null,"lastMessageId":null,
        "hasUnread":false,"unreadCount":0,"lastViewedAt":0,"lastActivityAt":t,"lastReadSeq":0,"lastIncomingSeq":0,"lastMessageAt":0,
        "awaitingUserResponse":null,"notifyOnUpdatesEnabled":true,"isHiddenFromSidebar":false,"isGroup":kind=="room","memberIds":[],"bot":null});
    patch(
        &mut v,
        p,
        &[
            "description",
            "title",
            "avatarShape",
            "avatarColor",
            "memberIds",
        ],
    );
    v
}
