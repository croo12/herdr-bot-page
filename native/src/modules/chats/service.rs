use crate::{
    domain::{need, now, patch, string_field},
    storage::{Repositories, Store},
};
use anyhow::Result;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatEdit {
    Profile,
    Hidden,
    Notifications,
    Read,
    Unread,
}
pub(crate) fn find_chats(store: &Store, query: &str) -> Result<Value> {
    store.tx(|repos, _| {
        let query = query.to_lowercase();
        let chats = repos
            .chats
            .list()?
            .into_iter()
            .filter(|chat| {
                [
                    string_field(chat, "name"),
                    string_field(chat, "description"),
                ]
                .iter()
                .any(|text| text.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        Ok(json!(chats))
    })
}
pub(crate) fn edit_chat(store: &Store, edit: ChatEdit, params: &Value) -> Result<Value> {
    store.tx(|repos, events| {
        let chat_id = string_field(params, "chatId");
        let mut row = repos.chats.get(chat_id)?;
        match edit {
            ChatEdit::Profile => {
                if params.get("name").is_some() {
                    need(params, "name")?;
                }
                patch(
                    &mut row,
                    params,
                    &["name", "description", "title", "avatarColor", "avatarShape"],
                );
            }
            ChatEdit::Hidden => row["isHiddenFromSidebar"] = bool_field(params, "isHidden")?.into(),
            ChatEdit::Notifications => {
                row["notifyOnUpdatesEnabled"] = bool_field(params, "isEnabled")?.into()
            }
            ChatEdit::Read | ChatEdit::Unread => mark_read(repos, &mut row, edit, params)?,
        }
        repos.chats.save_changed(&row, events)?;
        if edit == ChatEdit::Read {
            Ok(json!({"lastReadSeq":row["lastReadSeq"]}))
        } else {
            Ok(row)
        }
    })
}
fn bool_field(params: &Value, key: &str) -> Result<bool> {
    params[key]
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("invalid_args: expected boolean {key}"))
}
fn mark_read(repos: &Repositories, row: &mut Value, edit: ChatEdit, params: &Value) -> Result<()> {
    let id = need(row, "id")?;
    let latest = repos.transcript.latest_seq(id)?;
    let through = if edit == ChatEdit::Read {
        params["throughSeq"]
            .as_i64()
            .filter(|sequence| *sequence >= 0)
            .ok_or_else(|| anyhow::anyhow!("invalid_args: invalid sequence"))?
            .clamp(row["lastReadSeq"].as_i64().unwrap_or(0).min(latest), latest)
    } else {
        latest
    };
    let unread = edit == ChatEdit::Unread && bool_field(params, "isUnread")?;
    let unread_count = repos.transcript.unread_after(id, through)?;
    if !unread {
        row["lastReadSeq"] = through.into();
        row["lastViewedAt"] = now().into();
    }
    row["hasUnread"] = (unread || unread_count > 0).into();
    row["unreadCount"] = unread_count.max(i64::from(unread)).into();
    Ok(())
}

pub(crate) fn profile_for_pane(store: &Store, params: &Value) -> Result<Value> {
    let pane = need(params, "paneId")?;
    store.tx(|repos, _| repos.chats.by_pane(pane))
}

pub(crate) fn update_profile_for_pane(store: &Store, mut params: Value) -> Result<Value> {
    let bot = profile_for_pane(store, &params)?;
    params["chatId"] = need(&bot, "id")?.into();
    edit_chat(store, ChatEdit::Profile, &params)
}
