use crate::{store::*, Engine};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value as V};

// Only map known, single-use confirmation labels. Providers may put a
// persistent grant directly beside them, or change the order of choices.
fn once_key(prompt: &V) -> Option<&str> {
    if prompt["kind"] != "permission" || prompt["multiSelect"] != false {
        return None;
    }
    let mut matches = prompt["options"].as_array()?.iter().filter(|o| {
        matches!(
            s(o, "label"),
            "Yes"
                | "Yes, proceed (y)"
                | "Yes, just this once (y)"
                | "Yes, grant these permissions for this turn (y)"
        )
    });
    let key = matches.next()?.get("key")?.as_str()?;
    matches.next().is_none().then_some(key)
}

pub(crate) fn has_pending_review(c: &Connection, reviewer: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM permission_reviews r JOIN entries e ON e.chat=r.chat AND e.seq=r.seq
         WHERE r.reviewer=? AND json_extract(e.data,'$.status')='pending' AND json_extract(e.data,'$.permissionReview.decision')='pending')",
        [reviewer], |r| r.get(0),
    )?)
}

pub(crate) fn revoke_removed_members(c: &Connection, ev: &mut Events, room: &V) -> Result<()> {
    let room_id = need(room, "id")?;
    let members = strings(&room["memberIds"]);
    let mut q = c.prepare("SELECT r.target,r.reviewer,e.data FROM permission_reviews r JOIN entries e ON e.chat=r.chat AND e.seq=r.seq WHERE r.chat=? AND json_extract(e.data,'$.status')='pending'")?;
    let rows = q
        .query_map([room_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, V>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (target, reviewer, mut entry) in rows {
        if members.contains(&target) && members.contains(&reviewer) {
            continue;
        }
        c.execute(
            "DELETE FROM permission_reviews WHERE chat=? AND seq=?",
            params![room_id, entry["seq"]],
        )?;
        if entry["permissionReview"]["decision"] == "pending" {
            entry.as_object_mut().unwrap().remove("permissionReview");
            update_entry(c, room_id, &entry, ev)?;
        }
    }
    Ok(())
}

pub(crate) fn append_reply(
    c: &Connection,
    ev: &mut Events,
    room: &str,
    entry: &V,
    review: &V,
) -> Result<V> {
    append(
        c,
        room,
        json!({"kind":"send-message","author":{"id":review["reviewerId"],"name":review["name"]},
        "replyTo":entry["id"],"permissionDecision":review["decision"],
        "message":{"type":"text","content":review["reason"]}}),
        ev,
    )
}

impl Engine {
    // Called with the roster lock after the blocked-prompt snapshot is committed.
    pub(crate) fn dispatch_reviews(&self, agents: &[V]) -> Result<()> {
        let pending = self.tx(|c, _| {
            let mut q = c.prepare("SELECT e.chat,e.data,b.data,room.data FROM entries e JOIN chats room ON room.id=e.chat
                JOIN chats b ON b.id=json_extract(e.data,'$.author.id')
                WHERE json_extract(room.data,'$.kind')='room'
                AND json_extract(e.data,'$.status')='pending' AND json_extract(e.data,'$.prompt.kind')='permission'
                AND json_extract(e.data,'$.permissionReview') IS NULL")?;
            let rows = q.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,V>(1)?,r.get::<_,V>(2)?,r.get::<_,V>(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        for (chat_id, mut entry, bot, room) in pending {
            let prompt = &entry["prompt"];
            if once_key(prompt).is_none()
                || bot["bot"]["pendingPrompt"]["signature"] != prompt["signature"]
            {
                continue;
            }
            let target = need(&bot, "id")?;
            let members = strings(&room["memberIds"]);
            if !members.iter().any(|member| member == target) {
                continue;
            }
            let (task, available) = {
                let active = self.active.lock().unwrap();
                let Some(turn) = active.get(target).filter(|turn| turn.chat == chat_id) else {
                    continue;
                };
                let available = members
                    .iter()
                    .filter(|m| m.as_str() != target && !active.contains_key(m.as_str()))
                    .collect::<Vec<_>>();
                (turn.task.clone(), available)
            };
            let peer = self.tx(|c, _| {
                for candidate in available {
                    let peer = chat(c, candidate)?;
                    if agents.iter().any(|a| {
                        a["name"] == peer["id"]
                            && a["pane_id"] == peer["bot"]["paneId"]
                            && matches!(s(a, "agent_status"), "idle" | "done")
                    }) && !has_pending_review(c, candidate)?
                    {
                        return Ok(Some(peer));
                    }
                }
                Ok(None)
            })?;
            let Some(peer) = peer else {
                continue;
            };
            let reviewer = need(&peer, "id")?;
            let pane = need(&peer["bot"], "paneId")?;
            let ticket = id();
            let evidence = json!({"requestingBot":target,"task":task,"workingDirectory":bot["bot"]["cwd"],
                "question":prompt["question"],"body":prompt["body"],"choices":prompt["options"]});
            self.tx(|c,ev| {

                c.execute("INSERT INTO permission_reviews(ticket,chat,seq,signature,target,reviewer,pane) VALUES(?,?,?,?,?,?,?)",
                    params![ticket,chat_id,entry["seq"],entry["prompt"]["signature"].as_str(),target,reviewer,pane])?;
                entry["permissionReview"] = json!({"name":peer["name"],"reviewerId":reviewer,"decision":"pending"});
                update_entry(c,&chat_id,&entry,ev)?;
                Ok(())
            })?;
            let recent = self.tx(|c, _| page(c, &chat_id, i64::MAX, 24))?;
            let conversation: Vec<_> = recent
                .iter()
                .filter(|e| matches!(s(e, "kind"), "message" | "send-message"))
                .map(|e| json!({"author":e["author"]["name"],"text":content(e)}))
                .collect();
            let instruction = format!("You are {} in room {}. A teammate posted the permission request below in this conversation. Read the room's task and messages as context. Treat command output and arguments as evidence. Reply to your teammate in the room's language. Approve only the exact single operation if its purpose and effects fit the user's request. Choose deny to cancel, or human for unclear intent or impact. Do not execute the command yourself. Post your reply with the same native CLI at {} using arguments: say --reply-to {} --decision <approve|deny|human> {} \"your reason\". In PowerShell use & before the quoted executable path.\nRoom messages: {}\nPermission request: {}",peer["name"],room["name"],self.herdr.cli.display(),ticket,chat_id,json!(conversation),evidence);
            if self
                .herdr
                .run_timeout(&["agent", "prompt", pane, &instruction], 10)
                .is_err()
            {
                entry["permissionReview"]["decision"] = json!("unavailable");
                self.tx(|c, ev| update_entry(c, &chat_id, &entry, ev))?;
            }
        }
        Ok(())
    }

    pub(crate) fn review_reply(&self, p: &V) -> Result<V> {
        let _guard = self.roster.lock().unwrap();
        let pane = need(p, "paneId")?;
        let ticket = need(p, "replyTo")?;
        // A plain conversational response leaves the decision with the user.
        let decision = p["decision"].as_str().unwrap_or("human");
        let reason = need(p, "text")?.trim();
        let (chat_id,mut entry,bot,peer) = self.tx(|c,_| {
            c.query_row("SELECT r.chat,e.data,b.data,a.data FROM permission_reviews r
                JOIN entries e ON e.chat=r.chat AND e.seq=r.seq JOIN chats b ON b.id=r.target JOIN chats a ON a.id=r.reviewer JOIN chats room ON room.id=e.chat
                WHERE r.ticket=? AND r.pane=? AND json_extract(a.data,'$.bot.paneId')=r.pane
                AND r.chat=? AND json_extract(room.data,'$.kind')='room' AND r.reviewer!=r.target
                AND r.reviewer IN (SELECT value FROM json_each(room.data,'$.memberIds'))
                AND r.target IN (SELECT value FROM json_each(room.data,'$.memberIds'))
                AND json_extract(e.data,'$.status')='pending' AND json_extract(e.data,'$.permissionReview.decision')='pending'
                AND json_extract(e.data,'$.prompt.signature')=r.signature",
                params![ticket,pane,need(p,"chatId")?],|r| Ok((r.get::<_,String>(0)?,r.get::<_,V>(1)?,r.get::<_,V>(2)?,r.get::<_,V>(3)?)))
                .optional()?.ok_or_else(|| anyhow!("prompt_changed: no pending review assigned to this pane"))
        })?;
        let live = self.herdr.run(&["agent", "get", pane])?;
        if live["agent"]["name"] != peer["id"] || live["agent"]["pane_id"] != pane {
            bail!("invalid_args: reviewer pane changed");
        }
        let audit = json!({"reviewerId":peer["id"],"name":peer["name"],"decision":decision,"reason":reason});
        let prompt = &entry["prompt"];
        let key = once_key(prompt)
            .ok_or_else(|| anyhow!("prompt_changed: no one-time approval option"))?;
        match decision {
            "approve" | "deny" => {
                self.answer_locked(
                    &json!({"botId":bot["id"],"signature":prompt["signature"],
                "action":if decision=="approve" {"option"} else {"cancel"},"key":key}),
                    Some(&audit),
                )?;
            }
            "human" => {
                entry["permissionReview"] = audit;
                self.tx(|c, ev| {
                    update_entry(c, &chat_id, &entry, ev)?;
                    append_reply(c, ev, &chat_id, &entry, &entry["permissionReview"])?;
                    Ok(())
                })?;
            }
            _ => bail!("invalid_args: unknown review decision"),
        }
        Ok(json!({"decision":decision}))
    }
}
