use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value as V};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn id() -> String {
    Uuid::new_v4().to_string()
}
pub fn s<'a>(v: &'a V, k: &str) -> &'a str {
    v[k].as_str().unwrap_or_default()
}
pub fn need<'a>(v: &'a V, k: &str) -> Result<&'a str> {
    v[k].as_str()
        .filter(|x| !x.trim().is_empty())
        .ok_or_else(|| anyhow!("invalid_args: missing {k}"))
}
pub fn strings(v: &V) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}
pub fn patch(to: &mut V, from: &V, keys: &[&str]) {
    for k in keys {
        if let Some(v) = from.get(k) {
            to[k] = v.clone();
        }
    }
}
pub fn open(home: &Path) -> Result<Connection> {
    let c = Connection::open(home.join("chat.sqlite3"))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
    CREATE TABLE IF NOT EXISTS chats(id TEXT PRIMARY KEY, data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS entries(chat TEXT REFERENCES chats(id) ON DELETE CASCADE, seq INTEGER, data TEXT NOT NULL, PRIMARY KEY(chat,seq));
    CREATE TABLE IF NOT EXISTS config(key TEXT PRIMARY KEY,data TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS requests(key TEXT PRIMARY KEY, chat TEXT REFERENCES chats(id) ON DELETE CASCADE);
    CREATE TABLE IF NOT EXISTS permission_reviews(
        ticket TEXT PRIMARY KEY, chat TEXT NOT NULL, seq INTEGER NOT NULL, signature TEXT NOT NULL,
        target TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
        reviewer TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE, pane TEXT NOT NULL,
        UNIQUE(chat,seq), FOREIGN KEY(chat,seq) REFERENCES entries(chat,seq) ON DELETE CASCADE
    );")?;
    c.execute(
        "UPDATE chats SET data=json_set(data,'$.isRunning',json('false'),'$.currentActivity',null)
        WHERE json_extract(data,'$.kind')='room' AND json_extract(data,'$.isRunning')=1",
        [],
    )?;
    Ok(c)
}
pub fn list(c: &Connection) -> Result<Vec<V>> {
    let mut q = c.prepare("SELECT data FROM chats ORDER BY rowid")?;
    let rows = q
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}
pub fn chat(c: &Connection, id: &str) -> Result<V> {
    c.query_row("SELECT data FROM chats WHERE id=?", [id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| anyhow!("unknown_chat: {id}"))
}
pub fn save(c: &Connection, v: &V) -> Result<()> {
    c.execute(
        "INSERT INTO chats VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
        params![need(v, "id")?, v],
    )?;
    Ok(())
}
pub fn settings(c: &Connection) -> Result<V> {
    Ok(c.query_row("SELECT data FROM config WHERE key='settings'",[],|r|r.get(0)).optional()?.unwrap_or_else(||json!({"defaultWorkingFolder":null,"defaultProvider":null,"defaultPermissionMode":null,"terminal":null})))
}
pub fn new_chat(id: &str, kind: &str, p: &V) -> V {
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
pub fn content(e: &V) -> &str {
    e["content"]
        .as_str()
        .or(e["message"]["content"].as_str())
        .or(e["prompt"]["question"].as_str())
        .unwrap_or_default()
}
pub fn page(c: &Connection, chat: &str, before: i64, limit: i64) -> Result<Vec<V>> {
    let mut q =
        c.prepare("SELECT data FROM entries WHERE chat=? AND seq<? ORDER BY seq DESC LIMIT ?")?;
    let mut entries = q
        .query_map(params![chat, before, limit], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    entries.reverse();
    Ok(entries)
}
pub type Events = Vec<(&'static str, V)>;
pub fn change(c: &Connection, v: &V, events: &mut Events) -> Result<()> {
    save(c, v)?;
    events.push(("chat.changed", v.clone()));
    Ok(())
}
pub fn append(c: &Connection, chat_id: &str, mut e: V, events: &mut Events) -> Result<V> {
    let mut v = chat(c, chat_id)?;
    let seq: i64 = c.query_row(
        "SELECT coalesce(max(seq),0)+1 FROM entries WHERE chat=?",
        [chat_id],
        |r| r.get(0),
    )?;
    let t = now();
    e["seq"] = seq.into();
    e["id"] = id().into();
    e["timestampMs"] = t.into();
    c.execute(
        "INSERT INTO entries VALUES(?,?,?)",
        params![chat_id, seq, e],
    )?;
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
pub fn update_entry(c: &Connection, chat_id: &str, e: &V, events: &mut Events) -> Result<()> {
    c.execute(
        "UPDATE entries SET data=? WHERE chat=? AND seq=?",
        params![e, chat_id, e["seq"].as_i64()],
    )?;
    events.push((
        "transcript.entryUpdated",
        json!({"chatId":chat_id,"entry":e}),
    ));
    Ok(())
}
