use crate::{herdr, store::*, turns::Job, Engine};
use anyhow::{bail, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value as V};
use std::{path::PathBuf, sync::Arc};

impl Engine {
    pub(crate) fn dispatch(self: &Arc<Self>, method: &str, p: V) -> Result<V> {
        match method {
            "bot.create"
            | "bot.quickCreate"
            | "bot.retrySetup"
            | "bot.updateLaunchSettings"
            | "bot.listAdoptable"
            | "chat.delete" => return self.roster_call(method, p),
            "bot.listModels" => {
                let provider = need(&p, "provider")?;
                return Ok(herdr::models(provider).unwrap_or_else(
                    |e| json!({"models":[],"source":"unavailable","error":format!("{e:#}")}),
                ));
            }
            "bot.focus" => {
                let v = self.tx(|c, _| chat(c, need(&p, "botId")?))?;
                self.herdr.focus(need(&v["bot"], "paneId")?)?;
                return Ok(V::Null);
            }
            "prompt.answer" => return self.answer(&p),
            "message.send" => return self.send(&p),
            "cli.say" | "cli.pass" | "cli.message" | "cli.profile" | "cli.whoami"
            | "cli.profileUpdate" => return self.bot_cli(method, p),
            "directory.list" => return directory(s(&p, "path")),
            _ => {}
        }
        self.tx(|c, events| self.data_call(c, events, method, &p))
    }

    fn data_call(
        &self,
        c: &rusqlite::Connection,
        events: &mut Events,
        method: &str,
        p: &V,
    ) -> Result<V> {
        let chat_id = s(p, "chatId");
        match method {
            "identity.get" => Ok(json!({"userName": self.user})),
            "host.status" => {
                let chats = list(c)?;
                let bots = chats.iter().filter(|v| v["kind"] == "bot").count();
                let running: Vec<_> = self.queues.lock().unwrap().keys().cloned().collect();
                Ok(
                    json!({"home":self.home,"session":self.herdr.session,"bots":bots,"rooms":chats.len()-bots,"runningChatIds":running}),
                )
            }
            "chat.list" | "chat.search" => {
                let query = s(p, "query").to_lowercase();
                let chats = list(c)?
                    .into_iter()
                    .filter(|v| {
                        [s(v, "name"), s(v, "description")]
                            .iter()
                            .any(|text| text.to_lowercase().contains(&query))
                    })
                    .collect::<Vec<_>>();
                Ok(json!(chats))
            }
            "settings.get" => settings(c),
            "settings.update" => {
                let mut value = settings(c)?;
                patch(
                    &mut value,
                    p,
                    &[
                        "defaultWorkingFolder",
                        "defaultProvider",
                        "defaultPermissionMode",
                        "terminal",
                    ],
                );
                c.execute(
                    "INSERT OR REPLACE INTO config VALUES('settings',?)",
                    [&value],
                )?;
                Ok(value)
            }
            "bot.defaults" => self.defaults(c),
            "room.create" | "room.setMembers" => update_room(c, events, method, p),
            "chat.updateProfile"
            | "chat.setHidden"
            | "chat.setNotifyOnUpdates"
            | "chat.setUnread"
            | "chat.markRead" => {
                let mut row = chat(c, chat_id)?;
                match method {
                    "chat.updateProfile" => {
                        if p.get("name").is_some() {
                            need(p, "name")?;
                        }
                        patch(
                            &mut row,
                            p,
                            &["name", "description", "title", "avatarColor", "avatarShape"],
                        );
                    }
                    "chat.setHidden" => {
                        row["isHiddenFromSidebar"] = bool_field(p, "isHidden")?.into()
                    }
                    "chat.setNotifyOnUpdates" => {
                        row["notifyOnUpdatesEnabled"] = bool_field(p, "isEnabled")?.into()
                    }
                    _ => mark_read(c, &mut row, method, p)?,
                }
                change(c, &row, events)?;
                if method == "chat.markRead" {
                    Ok(json!({"lastReadSeq":row["lastReadSeq"]}))
                } else {
                    Ok(row)
                }
            }
            "transcript.page" => {
                chat(c, chat_id)?;
                let limit = p["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
                let mut rows = page(
                    c,
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
            }
            "transcript.thread" => {
                let root = need(p, "rootId")?;
                let mut query=c.prepare("SELECT data FROM entries WHERE chat=? AND (json_extract(data,'$.id')=? OR json_extract(data,'$.replyTo')=?) ORDER BY seq")?;
                let rows = query
                    .query_map(params![chat_id, root, root], |r| r.get::<_, V>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(json!({"entries":rows}))
            }
            "message.react" => {
                let mut entry: V = c.query_row(
                    "SELECT data FROM entries WHERE chat=? AND json_extract(data,'$.id')=?",
                    params![chat_id, need(p, "entryId")?],
                    |r| r.get(0),
                )?;
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
            }
            "message.search" => search_messages(c, s(p, "query")),
            _ => bail!("unknown_method: {method}"),
        }
    }
    pub(crate) fn defaults(&self, c: &rusqlite::Connection) -> Result<V> {
        let v = settings(c)?;
        let last = list(c)?
            .into_iter()
            .rev()
            .find_map(|v| v["bot"]["cwd"].as_str().map(String::from));
        Ok(
            json!({"cwd":std::env::var("HERDR_BOT_DEFAULT_CWD").ok().or_else(||v["defaultWorkingFolder"].as_str().map(String::from)).or(last).unwrap_or_default(),
            "provider":v["defaultProvider"].as_str().map(String::from).or_else(||std::env::var("HERDR_BOT_DEFAULT_KIND").ok()).unwrap_or("claude".into()),
            "permissionMode":v["defaultPermissionMode"].as_str().unwrap_or("ask"),"homeDir":dirs::home_dir()}),
        )
    }
    fn send(self: &Arc<Self>, p: &V) -> Result<V> {
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
        let (entry, new) = self.tx(|c, ev| {
            if let Some(nonce) = p["clientNonce"].as_str() {
                let old = c.query_row(
                    "SELECT data FROM entries WHERE chat=? AND json_extract(data,'$.clientNonce')=?",
                    params![chat_id,nonce], |r|r.get::<_,V>(0)
                ).optional()?;
                if let Some(row) = old { return Ok((row,false)); }
            }
            let mut entry = json!({"kind":"message","role":"user","author":{"id":"user","name":self.user},"content":text,"isStreaming":false});
            patch(&mut entry,p,&["clientNonce","replyTo","richText"]);
            Ok((append(c,chat_id,entry,ev)?,true))
        })?;
        if new {
            self.enqueue(
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
}
fn bool_field(p: &V, key: &str) -> Result<bool> {
    p[key]
        .as_bool()
        .ok_or_else(|| anyhow::anyhow!("invalid_args: expected boolean {key}"))
}
fn update_room(c: &rusqlite::Connection, ev: &mut Events, method: &str, p: &V) -> Result<V> {
    if let Some(existing) = request_chat(c, p)? {
        return Ok(json!({"chat":existing}));
    }
    let raw = p["memberIds"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("invalid_args: memberIds must be an array"))?;
    let members = strings(&p["memberIds"]);
    if members.len() != raw.len() {
        bail!("invalid_args: invalid member ID")
    }
    if members.len() > 6 {
        bail!("too_many_members: maximum six bots")
    }
    let mut seen = std::collections::HashSet::new();
    for member in &members {
        if chat(c, member)?["kind"] != "bot" || !seen.insert(member) {
            bail!("invalid_args: invalid room member {member}")
        }
    }
    if method == "room.create" {
        need(p, "name")?;
        let room = new_chat(&format!("room-{}", id()), "room", p);
        save(c, &room)?;
        remember(c, p, &room)?;
        ev.push(("chat.listChanged", json!(list(c)?)));
        Ok(json!({"chat":room}))
    } else {
        let mut room = chat(c, need(p, "roomId")?)?;
        if room["kind"] != "room" {
            bail!("unknown_room: expected a room")
        }
        room["memberIds"] = json!(members);
        change(c, &room, ev)?;
        Ok(room)
    }
}
fn mark_read(c: &rusqlite::Connection, row: &mut V, method: &str, p: &V) -> Result<()> {
    let id = need(row, "id")?;
    let latest: i64 = c.query_row(
        "SELECT coalesce(max(seq),0) FROM entries WHERE chat=?",
        [id],
        |r| r.get(0),
    )?;
    let through = if method == "chat.markRead" {
        p["throughSeq"]
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| anyhow::anyhow!("invalid_args: invalid sequence"))?
            .clamp(row["lastReadSeq"].as_i64().unwrap_or(0).min(latest), latest)
    } else {
        latest
    };
    let unread = method == "chat.setUnread" && bool_field(p, "isUnread")?;
    let n:i64=c.query_row("SELECT count(*) FROM entries WHERE chat=? AND seq>? AND json_extract(data,'$.kind')!='message'",params![id,through],|r|r.get(0))?;
    if !unread {
        row["lastReadSeq"] = through.into();
        row["lastViewedAt"] = now().into();
    }
    row["hasUnread"] = (unread || n > 0).into();
    row["unreadCount"] = n.max(i64::from(unread)).into();
    Ok(())
}
fn search_messages(c: &rusqlite::Connection, query: &str) -> Result<V> {
    let query = query.trim().to_lowercase();
    let mut hits = vec![];
    if query.is_empty() {
        return Ok(json!(hits));
    }
    let mut statement=c.prepare("SELECT c.data,e.data FROM entries e JOIN chats c ON c.id=e.chat ORDER BY json_extract(e.data,'$.timestampMs') DESC")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let e: V = row.get(1)?;
        let text = content(&e);
        if !matches!(s(&e, "kind"), "message" | "send-message")
            || !text.to_lowercase().contains(&query)
        {
            continue;
        }
        let chat: V = row.get(0)?;
        hits.push(
            json!({"chatId":chat["id"],"chatName":chat["name"],"entryId":e["id"],
            "role":if e["kind"]=="message"{"user"}else{"assistant"},"author":e["author"]["name"],
            "timestampMs":e["timestampMs"],"snippet":text.chars().take(240).collect::<String>()}),
        );
        if hits.len() >= 100 {
            break;
        }
    }
    Ok(json!(hits))
}
pub fn request_chat(c: &rusqlite::Connection, p: &V) -> Result<Option<V>> {
    if let Some(r) = p["requestId"].as_str() {
        return Ok(c
            .query_row(
                "SELECT c.data FROM requests r JOIN chats c ON c.id=r.chat WHERE r.key=?",
                [r],
                |r| r.get(0),
            )
            .optional()?);
    }
    Ok(None)
}
pub fn remember(c: &rusqlite::Connection, p: &V, v: &V) -> Result<()> {
    if let Some(r) = p["requestId"].as_str() {
        c.execute(
            "INSERT INTO requests VALUES(?,?)",
            params![r, need(v, "id")?],
        )?;
    }
    Ok(())
}
pub(crate) fn expand(path: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    if path == "~" || path.is_empty() {
        home
    } else if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        home.join(rest)
    } else {
        home.join(path)
    }
}
fn directory(raw: &str) -> Result<V> {
    let path = expand(raw.trim());
    let exists = path.is_dir();
    let (parent, prefix) = if raw.is_empty() || raw.ends_with(['/', '\\']) {
        (path.as_path(), "")
    } else {
        (
            path.parent().unwrap_or(&path),
            path.file_name().and_then(|x| x.to_str()).unwrap_or(""),
        )
    };
    let mut entries = vec![];
    if let Ok(rd) = parent.read_dir() {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir()
                && (!name.starts_with('.') || prefix.starts_with('.'))
                && name.to_lowercase().starts_with(&prefix.to_lowercase())
            {
                entries.push(e.path());
            }
        }
    }
    entries.sort();
    entries.truncate(50);
    Ok(
        json!({"exists":exists,"isHome":exists&&path.canonicalize().ok()==dirs::home_dir().and_then(|x|x.canonicalize().ok()),"entries":entries}),
    )
}
