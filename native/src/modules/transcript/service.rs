use crate::modules::transcript::content;
use crate::modules::turns;
use crate::{
    domain::{id, need, now, patch, s, strings, Events},
    modules::{chats::change, turns::Job},
    storage::Repositories,
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};
use std::sync::Arc;
pub(crate) fn transcript_page(engine: &Engine, p: &V) -> Result<V> {
    let chat_id = s(p, "chatId");
    engine.tx(|c, _| {
        c.chats.get(chat_id)?;
        let limit = p["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
        let mut rows = c.transcript.page(
            chat_id,
            p["beforeSeq"].as_i64().unwrap_or(i64::MAX),
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
pub(crate) fn transcript_thread(engine: &Engine, p: &V) -> Result<V> {
    engine.tx(|c, _| Ok(json!({"entries":c.transcript.thread(s(p,"chatId"),need(p,"rootId")?)?})))
}
pub(crate) fn react(engine: &Engine, p: &V) -> Result<V> {
    let chat_id = s(p, "chatId");
    engine.tx(|c, events| {
        let mut entry = c.transcript.by_id(chat_id, need(p, "entryId")?)?;
        let mut reactions = entry["reactions"].as_array().cloned().unwrap_or_default();
        let reaction = json!({"by":"me","emoji":need(p,"emoji")?});
        if let Some(i) = reactions.iter().position(|v| v == &reaction) {
            reactions.remove(i);
        } else {
            reactions.push(reaction);
        }
        entry["reactions"] = json!(reactions);
        update_entry(c, chat_id, &entry, events)?;
        Ok(json!({"entry":entry}))
    })
}
pub(crate) fn search_messages(engine: &Engine, query: &str) -> Result<V> {
    engine.tx(|c, _| {
        let query = query.trim().to_lowercase();
        let mut hits = vec![];
        if query.is_empty() {
            return Ok(json!(hits));
        }
        c.transcript.visit_messages(|chat, e| {
            let text = content(&e);
            if !text.to_lowercase().contains(&query) {
                return true;
            }
            hits.push(
                json!({"chatId":chat["id"],"chatName":chat["name"],"entryId":e["id"],
        "role":if e["kind"]=="message"{"user"}else{"assistant"},"author":e["author"]["name"],
        "timestampMs":e["timestampMs"],"snippet":text.chars().take(240).collect::<String>()}),
            );
            hits.len() < 100
        })?;
        Ok(json!(hits))
    })
}
pub(crate) fn send(engine: &Arc<Engine>, p: &V) -> Result<V> {
    let chat_id = need(p, "chatId")?;
    let mut text = s(p, "text").to_owned();
    let paths = strings(&p["attachmentPaths"]);
    if !paths.is_empty() {
        text.push_str("\n\n");
        text += &paths
            .iter()
            .map(|x| format!("(attached: {x})"))
            .collect::<Vec<_>>()
            .join("\n");
    }
    if text.trim().is_empty() {
        bail!("invalid_args: message is empty")
    }
    let (entry, new) = engine.tx(|c, ev| {
        if let Some(nonce) = p["clientNonce"].as_str() {
            if let Some(row) = c.transcript.by_nonce(chat_id, nonce)? {
                return Ok((row, false));
            }
        }
        let mut entry = json!({
            "kind": "message", "role": "user", "author": {"id": "user", "name": engine.user},
            "content": text, "isStreaming": false
        });
        patch(&mut entry, p, &["clientNonce", "replyTo", "richText"]);
        Ok((append(c, chat_id, entry, ev)?, true))
    })?;
    if new {
        turns::enqueue(
            engine,
            chat_id,
            Job {
                seed: text,
                depth: 0,
                exclude: None,
            },
        );
    }
    Ok(json!({"entryId":entry["id"]}))
}
pub(crate) fn append(c: &Repositories, chat_id: &str, mut e: V, events: &mut Events) -> Result<V> {
    let mut v = c.chats.get(chat_id)?;
    let seq = c.transcript.latest_seq(chat_id)? + 1;
    let t = now();
    e["seq"] = seq.into();
    e["id"] = id().into();
    e["timestampMs"] = t.into();
    c.transcript.insert(chat_id, seq, &e)?;
    v["updatedAt"] = t.into();
    v["lastActivityAt"] = t.into();
    v["lastMessageAt"] = t.into();
    v["lastMessagePreview"] = content(&e).chars().take(180).collect::<String>().into();
    v["lastMessageId"] = e["id"].clone();
    if e["kind"] != "message" {
        v["lastIncomingSeq"] = seq.into();
        v["hasUnread"] = true.into();
        v["unreadCount"] = (v["unreadCount"].as_i64().unwrap_or(0) + 1).into();
    }
    events.push((
        "transcript.entryAppended",
        json!({"chatId":chat_id,"entry":e}),
    ));
    change(c, &v, events)?;
    Ok(e)
}
pub(crate) fn update_entry(
    c: &Repositories,
    chat_id: &str,
    e: &V,
    events: &mut Events,
) -> Result<()> {
    c.transcript.update(chat_id, e)?;
    events.push((
        "transcript.entryUpdated",
        json!({"chatId":chat_id,"entry":e}),
    ));
    Ok(())
}
