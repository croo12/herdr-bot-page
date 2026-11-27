use crate::{
    domain::{need, strings, ChatJob, Events},
    infrastructure::AgentSessions,
    storage::{Repositories, Store},
};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
#[derive(PartialEq, Eq)]
pub(crate) enum AgentAction {
    Say,
    Pass,
    Delegate,
    Direct,
}

fn post_message(
    repos: &Repositories,
    events: &mut Events,
    chat: &Value,
    bot: &Value,
    params: &Value,
    depth: u8,
    delegate: bool,
) -> Result<Value> {
    let chat_id = need(chat, "id")?;
    let bot_id = need(bot, "id")?;
    let text = need(params, "text")?;
    anyhow::ensure!(
        chat["archived"] != true && strings(&chat["memberIds"]).iter().any(|id| id == bot_id),
        "invalid_args: bot is not a current participant"
    );
    let mut entry = json!({
        "kind":"send-message",
        "author":{"id":bot_id,"name":bot["name"]},
        "message":{"type":"text","content":text}
    });
    if let Some(parent) = params["replyTo"].as_str() {
        anyhow::ensure!(
            repos.transcript.contains(chat_id, parent)?,
            "invalid_args: reply target is not in this chat"
        );
        entry["replyTo"] = parent.into();
    }
    let entry = repos.append_entry(chat_id, entry, events)?;
    repos
        .inbox
        .deliver(chat, &entry, Some(bot_id), depth, delegate)?;
    Ok(entry)
}
pub(crate) fn agent_action(
    store: &Store,
    sessions: &AgentSessions,
    enqueue_chat: &dyn Fn(&str, ChatJob),
    action: AgentAction,
    params: Value,
) -> Result<Value> {
    let pane = need(&params, "paneId")?;
    let bot = store.tx(|repos, _| repos.chats.by_pane(pane))?;
    let bot_id = need(&bot, "id")?;
    let requested = need(&params, "chatId")?;
    let mut active = sessions.active.lock().unwrap();
    if action == AgentAction::Direct && !active.contains_key(bot_id) {
        let result = store.tx(|repos, events| {
            let dm = repos.chats.open_direct(requested, Some(bot_id), events)?;
            let entry = post_message(repos, events, &dm, &bot, &params, 0, false)?;
            Ok(json!({"chatId":dm["id"],"entryId":entry["id"]}))
        })?;
        drop(active);
        store.schedule_inbox(enqueue_chat)?;
        return Ok(result);
    }
    let turn = active
        .get_mut(bot_id)
        .ok_or_else(|| anyhow!("bot_not_ready: no open turn for {bot_id}"))?;
    let cross = matches!(action, AgentAction::Delegate | AgentAction::Direct);
    let target_chat = store.tx(|repos, events| {
        let chat = repos.chats.get(requested)?;
        if cross && chat["kind"] == "bot" {
            if turn.depth >= 2 || turn.delegated {
                bail!("invalid_args: delegation is not available for this turn")
            }
            return repos.chats.open_direct(requested, Some(bot_id), events);
        }
        if !cross && requested == bot_id && turn.chat != requested {
            let active_chat = repos.chats.get(&turn.chat)?;
            if active_chat["kind"] == "direct" && active_chat["includesUser"] == true {
                return Ok(active_chat);
            }
        }
        Ok(chat)
    })?;
    let target = need(&target_chat, "id")?;
    if target_chat["archived"] == true {
        bail!("invalid_args: archived conversation")
    }
    if !cross && target != turn.chat {
        bail!("invalid_args: wrong chat for this turn")
    }
    if cross
        && (turn.depth >= 2
            || turn.delegated
            || target == turn.chat
            || target == bot_id
            || (target_chat["kind"] == "direct" && target_chat["includesUser"] == true)
            || (target_chat["kind"] != "bot"
                && !strings(&target_chat["memberIds"]).contains(&bot_id.to_owned())))
    {
        bail!("invalid_args: delegation is not available for this target or turn")
    }
    if turn.passed || turn.spoken >= 2 {
        bail!("bot_not_ready: turn has ended")
    }
    if action == AgentAction::Pass {
        if let Some(d) = &turn.delivery {
            store.tx(|repos, _| repos.inbox.finish(d))?;
        }
        turn.passed = true;
        return Ok(json!({"passed":true}));
    }
    let posted_entry = store.tx(|repos, events| {
        let chat = repos.chats.get(target)?;
        let entry = post_message(repos, events, &chat, &bot, &params, turn.depth + 1, cross)?;
        if let Some(delivery) = &turn.delivery {
            repos.inbox.finish(delivery)?;
        }
        Ok(entry)
    })?;
    turn.spoken += 1;
    if cross {
        turn.delegated = true;
        let from = turn.chat.clone();
        drop(active);
        store.notice(&from, &format!("{bot_id} sent a message to {target}."));
    }
    store.schedule_inbox(enqueue_chat)?;
    Ok(json!({"chatId":target,"entryId":posted_entry["id"]}))
}
