use crate::{
    domain::{need, now, patch, s, Events},
    storage::Repositories,
    Engine,
};
use anyhow::Result;
use serde_json::{json, Value as V};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatEdit {
    Profile,
    Hidden,
    Notifications,
    Read,
    Unread,
}
pub(crate) fn find_chats(engine: &Engine, query: &str) -> Result<V> {
    engine.tx(|c, _| {
        let query = query.to_lowercase();
        let chats = c
            .chats
            .list()?
            .into_iter()
            .filter(|v| {
                [s(v, "name"), s(v, "description")]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        Ok(json!(chats))
    })
}
pub(crate) fn edit_chat(engine: &Engine, edit: ChatEdit, p: &V) -> Result<V> {
    engine.tx(|c, events| {
        let chat_id = s(p, "chatId");
        let mut row = c.chats.get(chat_id)?;
        match edit {
            ChatEdit::Profile => {
                if p.get("name").is_some() {
                    need(p, "name")?;
                }
                patch(
                    &mut row,
                    p,
                    &["name", "description", "title", "avatarColor", "avatarShape"],
                );
            }
            ChatEdit::Hidden => row["isHiddenFromSidebar"] = bool_field(p, "isHidden")?.into(),
            ChatEdit::Notifications => {
                row["notifyOnUpdatesEnabled"] = bool_field(p, "isEnabled")?.into()
            }
            ChatEdit::Read | ChatEdit::Unread => mark_read(c, &mut row, edit, p)?,
        }
        change(c, &row, events)?;
        if edit == ChatEdit::Read {
            Ok(json!({"lastReadSeq":row["lastReadSeq"]}))
        } else {
            Ok(row)
        }
    })
}
fn bool_field(p: &V, key: &str) -> Result<bool> {
    p[key]
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("invalid_args: expected boolean {key}"))
}
fn mark_read(c: &Repositories, row: &mut V, edit: ChatEdit, p: &V) -> Result<()> {
    let id = need(row, "id")?;
    let latest = c.transcript.latest_seq(id)?;
    let through = if edit == ChatEdit::Read {
        p["throughSeq"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| anyhow::anyhow!("invalid_args: invalid sequence"))?
            .clamp(row["lastReadSeq"].as_i64().unwrap_or(0).min(latest), latest)
    } else {
        latest
    };
    let unread = edit == ChatEdit::Unread && bool_field(p, "isUnread")?;
    let n = c.transcript.unread_after(id, through)?;
    if !unread {
        row["lastReadSeq"] = through.into();
        row["lastViewedAt"] = now().into();
    }
    row["hasUnread"] = (unread || n > 0).into();
    row["unreadCount"] = n.max(i64::from(unread)).into();
    Ok(())
}
pub(crate) fn change(c: &Repositories, v: &V, events: &mut Events) -> Result<()> {
    c.chats.save(v)?;
    events.push(("chat.changed", v.clone()));
    Ok(())
}
