use crate::{
    domain::{id, need, now, patch, string_field, strings, ChatJob, Events},
    modules::{chats::change, transcript::content},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
pub(crate) fn notice(store: &Store, target: &str, text: &str) {
    let _ = store.tx(|repos, events| {
        append(
            repos,
            target,
            json!({"kind":"notice","content":text}),
            events,
        )
    });
}

pub(crate) fn transcript_page(store: &Store, params: &Value) -> Result<Value> {
    let chat_id = string_field(params, "chatId");
    store.tx(|repos, _| {
        repos.chats.get(chat_id)?;
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
    store.tx(|repos, _| Ok(json!({"entries":repos.transcript.thread(string_field(params,"chatId"),need(params,"rootId")?)?})))
}
pub(crate) fn react(store: &Store, params: &Value) -> Result<Value> {
    let chat_id = string_field(params, "chatId");
    store.tx(|repos, events| {
        let mut entry = repos.transcript.by_id(chat_id, need(params, "entryId")?)?;
        let mut reactions = entry["reactions"].as_array().cloned().unwrap_or_default();
        let reaction = json!({"by":"me","emoji":need(params,"emoji")?});
        if let Some(index) = reactions.iter().position(|existing| existing == &reaction) {
            reactions.remove(index);
        } else {
            reactions.push(reaction);
        }
        entry["reactions"] = json!(reactions);
        update_entry(repos, chat_id, &entry, events)?;
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
    enqueue_chat: &dyn Fn(&str, ChatJob),
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
    let (entry, new) = store.tx(|repos, events| {
        if let Some(nonce) = params["clientNonce"].as_str() {
            if let Some(row) = repos.transcript.by_nonce(chat_id, nonce)? {
                return Ok((row, false));
            }
        }
        let mut entry = json!({
            "kind": "message", "role": "user", "author": {"id": "user", "name": user},
            "content": text, "isStreaming": false
        });
        patch(&mut entry, params, &["clientNonce", "replyTo", "richText"]);
        Ok((append(repos, chat_id, entry, events)?, true))
    })?;
    if new {
        enqueue_chat(
            chat_id,
            ChatJob {
                seed: text,
                depth: 0,
                exclude: None,
            },
        );
    }
    Ok(json!({"entryId":entry["id"]}))
}
pub(crate) fn append(
    repos: &Repositories,
    chat_id: &str,
    mut entry: Value,
    events: &mut Events,
) -> Result<Value> {
    let mut chat = repos.chats.get(chat_id)?;
    let seq = repos.transcript.latest_seq(chat_id)? + 1;
    let timestamp = now();
    entry["seq"] = seq.into();
    entry["id"] = id().into();
    entry["timestampMs"] = timestamp.into();
    repos.transcript.insert(chat_id, seq, &entry)?;
    chat["updatedAt"] = timestamp.into();
    chat["lastActivityAt"] = timestamp.into();
    chat["lastMessageAt"] = timestamp.into();
    chat["lastMessagePreview"] = content(&entry).chars().take(180).collect::<String>().into();
    chat["lastMessageId"] = entry["id"].clone();
    if entry["kind"] != "message" {
        chat["lastIncomingSeq"] = seq.into();
        chat["hasUnread"] = true.into();
        chat["unreadCount"] = (chat["unreadCount"].as_i64().unwrap_or(0) + 1).into();
    }
    events.push((
        "transcript.entryAppended",
        json!({"chatId":chat_id,"entry":entry}),
    ));
    change(repos, &chat, events)?;
    Ok(entry)
}
pub(crate) fn update_entry(
    repos: &Repositories,
    chat_id: &str,
    entry: &Value,
    events: &mut Events,
) -> Result<()> {
    repos.transcript.update(chat_id, entry)?;
    events.push((
        "transcript.entryUpdated",
        json!({"chatId":chat_id,"entry":entry}),
    ));
    Ok(())
}
