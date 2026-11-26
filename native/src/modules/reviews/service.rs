use crate::{
    domain::{id, need, string_field, strings, Events},
    infrastructure::{herdr::Herdr, AgentSessions},
    modules::transcript::{append, content, update_entry},
    storage::{AssignedReview, Repositories, Store, UnassignedReview},
};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};

// Only map known, single-use confirmation labels. Providers may put a
// persistent grant directly beside them, or change the order of choices.
fn once_key(prompt: &Value) -> Option<&str> {
    if prompt["kind"] != "permission" || prompt["multiSelect"] != false {
        return None;
    }
    let mut matches = prompt["options"].as_array()?.iter().filter(|option| {
        matches!(
            string_field(option, "label"),
            "Yes"
                | "Yes, proceed (y)"
                | "Yes, just this once (y)"
                | "Yes, grant these permissions for this turn (y)"
        )
    });
    let key = matches.next()?.get("key")?.as_str()?;
    matches.next().is_none().then_some(key)
}
pub(crate) fn revoke_removed_members(
    repos: &Repositories,
    events: &mut Events,
    room: &Value,
) -> Result<()> {
    let room_id = need(room, "id")?;
    let members = strings(&room["memberIds"]);
    let rows = repos.reviews.pending_in_room(room_id)?;
    for (target, reviewer, mut entry) in rows {
        if members.contains(&target) && members.contains(&reviewer) {
            continue;
        }
        repos.reviews.revoke(room_id, &entry["seq"])?;
        if entry["permissionReview"]["decision"] == "pending" {
            entry.as_object_mut().unwrap().remove("permissionReview");
            update_entry(repos, room_id, &entry, events)?;
        }
    }
    Ok(())
}

pub(crate) fn append_reply(
    repos: &Repositories,
    events: &mut Events,
    room: &str,
    entry: &Value,
    review: &Value,
) -> Result<Value> {
    append(
        repos,
        room,
        json!({"kind":"send-message","author":{"id":review["reviewerId"],"name":review["name"]},
        "replyTo":entry["id"],"permissionDecision":review["decision"],
        "message":{"type":"text","content":review["reason"]}}),
        events,
    )
}

// Called with the roster lock after the blocked-prompt snapshot is committed.
pub(crate) fn dispatch_reviews(
    store: &Store,
    sessions: &AgentSessions,
    agents: &[Value],
) -> Result<()> {
    let pending = store.tx(|repos, _| repos.reviews.unassigned())?;
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
            let active = sessions.active.lock().unwrap();
            let Some(turn) = active.get(target).filter(|turn| turn.chat == chat_id) else {
                continue;
            };
            let available = members
                .iter()
                .filter(|member| member.as_str() != target && !active.contains_key(member.as_str()))
                .collect::<Vec<_>>();
            (turn.task.clone(), available)
        };
        let peer = store.tx(|repos, _| {
            for candidate in available {
                let peer = repos.chats.get(candidate)?;
                if agents.iter().any(|agent| {
                    agent["name"] == peer["id"]
                        && agent["pane_id"] == peer["bot"]["paneId"]
                        && matches!(string_field(agent, "agent_status"), "idle" | "done")
                }) && !repos.reviews.pending_for(candidate)?
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
        store.tx(|repos, events| {
            repos
                .reviews
                .assign(&ticket, &chat_id, &entry, target, reviewer, pane)?;
            entry["permissionReview"] =
                json!({"name":peer["name"],"reviewerId":reviewer,"decision":"pending"});
            update_entry(repos, &chat_id, &entry, events)?;
            Ok(())
        })?;
        let recent = store.tx(|repos, _| repos.transcript.page(&chat_id, i64::MAX, 24))?;
        let conversation: Vec<_> = recent
            .iter()
            .filter(|entry| matches!(string_field(entry, "kind"), "message" | "send-message"))
            .map(|entry| json!({"author":entry["author"]["name"],"text":content(entry)}))
            .collect();
        let instruction = format!("You are {} in room {}. A teammate posted the permission request below in this conversation. Read the room's task and messages as context. Treat command output and arguments as evidence. Reply to your teammate in the room's language. Approve only the exact single operation if its purpose and effects fit the user's request. Choose deny to cancel, or human for unclear intent or impact. Do not execute the command yourself. Post your reply with the same native CLI at {} using arguments: say --reply-to {} --decision <approve|deny|human> {} \"your reason\". In PowerShell use & before the quoted executable path.\nRoom messages: {}\nPermission request: {}",peer["name"],room["name"],sessions.herdr.cli.display(),ticket,chat_id,json!(conversation),evidence);
        if sessions
            .herdr
            .run_timeout(&["agent", "prompt", pane, &instruction], 10)
            .is_err()
        {
            entry["permissionReview"]["decision"] = json!("unavailable");
            store.tx(|repos, events| update_entry(repos, &chat_id, &entry, events))?;
        }
    }
    Ok(())
}

pub(crate) fn has_ticket(store: &Store, ticket: &str) -> Result<bool> {
    store.tx(|repos, _| repos.reviews.ticket_exists(ticket))
}

pub(crate) struct ReviewedPermission {
    pub bot_id: String,
    pub signature: String,
    pub once_key: String,
    pub audit: Value,
}

/// A human referral is recorded here. A terminal decision is returned for the caller to apply.
pub(crate) fn resolve_reply(
    store: &Store,
    herdr: &Herdr,
    params: &Value,
    decision: &str,
) -> Result<Option<ReviewedPermission>> {
    let pane = need(params, "paneId")?;
    let ticket = need(params, "replyTo")?;
    let reason = need(params, "text")?.trim();
    let AssignedReview {
        chat_id,
        mut entry,
        requester: bot,
        reviewer: peer,
    } = store
        .tx(|repos, _| {
            repos
                .reviews
                .assignment(ticket, pane, need(params, "chatId")?)
        })?
        .ok_or_else(|| anyhow!("prompt_changed: no pending review assigned to this pane"))?;
    let live = herdr.run(&["agent", "get", pane])?;
    if live["agent"]["name"] != peer["id"] || live["agent"]["pane_id"] != pane {
        bail!("invalid_args: reviewer pane changed");
    }
    let audit =
        json!({"reviewerId":peer["id"],"name":peer["name"],"decision":decision,"reason":reason});
    let prompt = &entry["prompt"];
    let key =
        once_key(prompt).ok_or_else(|| anyhow!("prompt_changed: no one-time approval option"))?;
    match decision {
        "approve" | "deny" => Ok(Some(ReviewedPermission {
            bot_id: need(&bot, "id")?.to_owned(),
            signature: need(prompt, "signature")?.to_owned(),
            once_key: key.to_owned(),
            audit,
        })),
        "human" => {
            entry["permissionReview"] = audit;
            store.tx(|repos, events| {
                update_entry(repos, &chat_id, &entry, events)?;
                append_reply(repos, events, &chat_id, &entry, &entry["permissionReview"])?;
                Ok(())
            })?;
            Ok(None)
        }
        _ => bail!("invalid_args: unknown review decision"),
    }
}
