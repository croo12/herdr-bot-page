use super::repository::{AssignedReview, UnassignedReview};
use crate::modules::prompts;
use crate::modules::transcript::content;
use crate::{
    domain::{id, need, s, strings, Events},
    modules::transcript::{append, update_entry},
    storage::Repositories,
    Engine,
};
use anyhow::{anyhow, bail, Result};
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
pub(crate) fn revoke_removed_members(c: &Repositories, ev: &mut Events, room: &V) -> Result<()> {
    let room_id = need(room, "id")?;
    let members = strings(&room["memberIds"]);
    let rows = c.reviews.pending_in_room(room_id)?;
    for (target, reviewer, mut entry) in rows {
        if members.contains(&target) && members.contains(&reviewer) {
            continue;
        }
        c.reviews.revoke(room_id, &entry["seq"])?;
        if entry["permissionReview"]["decision"] == "pending" {
            entry.as_object_mut().unwrap().remove("permissionReview");
            update_entry(c, room_id, &entry, ev)?;
        }
    }
    Ok(())
}

pub(crate) fn append_reply(
    c: &Repositories,
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

// Called with the roster lock after the blocked-prompt snapshot is committed.
pub(crate) fn dispatch_reviews(engine: &Engine, agents: &[V]) -> Result<()> {
    let pending = engine.tx(|c, _| c.reviews.unassigned())?;
    for UnassignedReview {
        chat_id,
        mut entry,
        requester: bot,
        room,
    } in pending
    {
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
            let active = engine.active.lock().unwrap();
            let Some(turn) = active.get(target).filter(|turn| turn.chat == chat_id) else {
                continue;
            };
            let available = members
                .iter()
                .filter(|m| m.as_str() != target && !active.contains_key(m.as_str()))
                .collect::<Vec<_>>();
            (turn.task.clone(), available)
        };
        let peer = engine.tx(|c, _| {
            for candidate in available {
                let peer = c.chats.get(candidate)?;
                if agents.iter().any(|a| {
                    a["name"] == peer["id"]
                        && a["pane_id"] == peer["bot"]["paneId"]
                        && matches!(s(a, "agent_status"), "idle" | "done")
                }) && !c.reviews.pending_for(candidate)?
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
        engine.tx(|c, ev| {
            c.reviews
                .assign(&ticket, &chat_id, &entry, target, reviewer, pane)?;
            entry["permissionReview"] =
                json!({"name":peer["name"],"reviewerId":reviewer,"decision":"pending"});
            update_entry(c, &chat_id, &entry, ev)?;
            Ok(())
        })?;
        let recent = engine.tx(|c, _| c.transcript.page(&chat_id, i64::MAX, 24))?;
        let conversation: Vec<_> = recent
            .iter()
            .filter(|e| matches!(s(e, "kind"), "message" | "send-message"))
            .map(|e| json!({"author":e["author"]["name"],"text":content(e)}))
            .collect();
        let instruction = format!("You are {} in room {}. A teammate posted the permission request below in this conversation. Read the room's task and messages as context. Treat command output and arguments as evidence. Reply to your teammate in the room's language. Approve only the exact single operation if its purpose and effects fit the user's request. Choose deny to cancel, or human for unclear intent or impact. Do not execute the command yourself. Post your reply with the same native CLI at {} using arguments: say --reply-to {} --decision <approve|deny|human> {} \"your reason\". In PowerShell use & before the quoted executable path.\nRoom messages: {}\nPermission request: {}",peer["name"],room["name"],engine.herdr.cli.display(),ticket,chat_id,json!(conversation),evidence);
        if engine
            .herdr
            .run_timeout(&["agent", "prompt", pane, &instruction], 10)
            .is_err()
        {
            entry["permissionReview"]["decision"] = json!("unavailable");
            engine.tx(|c, ev| update_entry(c, &chat_id, &entry, ev))?;
        }
    }
    Ok(())
}

pub(crate) fn review_reply(engine: &Engine, p: &V) -> Result<V> {
    let _guard = engine.roster.lock().unwrap();
    let pane = need(p, "paneId")?;
    let ticket = need(p, "replyTo")?;
    // A plain conversational response leaves the decision with the user.
    let decision = p["decision"].as_str().unwrap_or("human");
    let reason = need(p, "text")?.trim();
    let AssignedReview {
        chat_id,
        mut entry,
        requester: bot,
        reviewer: peer,
    } = engine
        .tx(|c, _| c.reviews.assignment(ticket, pane, need(p, "chatId")?))?
        .ok_or_else(|| anyhow!("prompt_changed: no pending review assigned to this pane"))?;
    let live = engine.herdr.run(&["agent", "get", pane])?;
    if live["agent"]["name"] != peer["id"] || live["agent"]["pane_id"] != pane {
        bail!("invalid_args: reviewer pane changed");
    }
    let audit =
        json!({"reviewerId":peer["id"],"name":peer["name"],"decision":decision,"reason":reason});
    let prompt = &entry["prompt"];
    let key =
        once_key(prompt).ok_or_else(|| anyhow!("prompt_changed: no one-time approval option"))?;
    match decision {
        "approve" | "deny" => {
            prompts::answer_locked(
                engine,
                &json!({"botId":bot["id"],"signature":prompt["signature"],
            "action":if decision=="approve" {"option"} else {"cancel"},"key":key}),
                Some(&audit),
            )?;
        }
        "human" => {
            entry["permissionReview"] = audit;
            engine.tx(|c, ev| {
                update_entry(c, &chat_id, &entry, ev)?;
                append_reply(c, ev, &chat_id, &entry, &entry["permissionReview"])?;
                Ok(())
            })?;
        }
        _ => bail!("invalid_args: unknown review decision"),
    }
    Ok(json!({"decision":decision}))
}
