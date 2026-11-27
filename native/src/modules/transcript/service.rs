use crate::{
    domain::{content, need, patch, string_field, strings, MessageSource},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
fn readable_chat(repos: &Repositories, params: &Value) -> Result<Value> {
    let chat_id = need(params, "chatId")?;
    let chat = repos.chats.get(chat_id)?;
    let caller = if let Some(pane) = params["paneId"].as_str().filter(|pane| !pane.is_empty()) {
        repos.bots.pane_owner(pane)?
    } else {
        None
    };
    let bot = caller.as_ref().map(crate::domain::bot::BotId::as_str);
    // The legacy bot destination resolves to the caller's conversation with that bot.
    let direct = if chat["kind"] == "bot" {
        repos
            .chats
            .direct(chat_id, bot.filter(|id| *id != chat_id))?
    } else {
        None
    };
    if !crate::domain::visible_to(direct.as_ref().unwrap_or(&chat), bot) {
        bail!("invalid_args: not a participant")
    }
    Ok(direct.unwrap_or(chat))
}
pub(crate) fn transcript_page(store: &Store, params: &Value) -> Result<Value> {
    store.tx(|repos, _| {
        let chat = readable_chat(repos, params)?;
        let chat_id = need(&chat, "id")?;
        let limit = params["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
        let mut rows = repos.transcript.page(
            chat_id,
            params["beforeSeq"].as_i64().unwrap_or(i64::MAX),
            limit + 1,
        )?;
        let mut result = json!({});
        if rows.len() > limit as usize {
            rows.remove(0);
            result["nextBeforeSeq"] = rows[0]["seq"].clone();
        }
        result["entries"] = json!(rows);
        Ok(result)
    })
}
pub(crate) fn transcript_thread(store: &Store, params: &Value) -> Result<Value> {
    store.tx(|repos, _| {
        let chat = readable_chat(repos, params)?;
        Ok(json!({"entries":repos.transcript.thread(need(&chat,"id")?,need(params,"rootId")?)?}))
    })
}
pub(crate) fn react(store: &Store, params: &Value) -> Result<Value> {
    store.tx(|repos, events| {
        let chat = readable_chat(repos, params)?;
        let chat_id = need(&chat, "id")?;
        let mut entry = repos.transcript.by_id(chat_id, need(params, "entryId")?)?;
        let mut reactions = entry["reactions"].as_array().cloned().unwrap_or_default();
        let reaction = json!({"by":"me","emoji":need(params,"emoji")?});
        if let Some(index) = reactions.iter().position(|existing| existing == &reaction) {
            reactions.remove(index);
        } else {
            reactions.push(reaction);
        }
        entry["reactions"] = json!(reactions);
        repos.update_entry(chat_id, &entry, events)?;
        Ok(json!({"entry":entry}))
    })
}
pub(crate) fn search_messages(store: &Store, query: &str) -> Result<Value> {
    store.tx(|repos, _| {
        let query = query.trim().to_lowercase();
        let mut hits = vec![];
        if query.is_empty() {
            return Ok(json!(hits));
        }
        repos.transcript.visit_messages(|chat, entry| {
            if chat["kind"]=="direct" && chat["includesUser"]!=true { return true; }
            let text = content(&entry);
            if !text.to_lowercase().contains(&query) {
                return true;
            }
            hits.push(
                json!({"chatId":chat["id"],"chatName":chat["name"],"entryId":entry["id"],
        "role":if entry["kind"]=="message"{"user"}else{"assistant"},"author":entry["author"]["name"],
        "timestampMs":entry["timestampMs"],"snippet":text.chars().take(240).collect::<String>()}),
            );
            hits.len() < 100
        })?;
        Ok(json!(hits))
    })
}
pub(crate) fn send(
    store: &Store,
    user: &str,
    wake_bot: &dyn Fn(&str),
    params: &Value,
) -> Result<Value> {
    let chat_id = need(params, "chatId")?;
    let mut text = string_field(params, "text").to_owned();
    let paths = strings(&params["attachmentPaths"]);
    if !paths.is_empty() {
        text.push_str("\n\n");
        text += &paths
            .iter()
            .map(|path| format!("(attached: {path})"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if text.trim().is_empty() {
        bail!("invalid_args: message is empty")
    }
    let (entry, recipients) = store.tx(|repos, events| {
        let mut chat = repos.chats.get(chat_id)?;
        if chat["kind"] == "bot" {
            chat = repos.chats.open_direct(chat_id, None, events)?;
        }
        if chat["kind"] == "direct" && chat["includesUser"] != true {
            bail!("invalid_args: user is not a participant")
        }
        let chat_id = need(&chat, "id")?;
        if chat["archived"] == true {
            bail!("invalid_args: archived conversation")
        }
        if let Some(nonce) = params["clientNonce"].as_str() {
            if let Some(row) = repos.transcript.by_nonce(chat_id, nonce)? {
                return Ok((row, Vec::new()));
            }
        }
        let mut entry = json!({
            "kind": "message", "role": "user", "author": {"id": "user", "name": user},
            "content": text, "isStreaming": false
        });
        patch(&mut entry, params, &["clientNonce", "replyTo", "richText"]);
        let entry = repos.append_entry(chat_id, entry, events)?;
        let recipients = repos.inbox.deliver(&chat, &entry, MessageSource::User, 0)?;
        Ok((entry, recipients))
    })?;
    for bot in recipients {
        wake_bot(&bot);
    }
    Ok(json!({"entryId":entry["id"]}))
}
