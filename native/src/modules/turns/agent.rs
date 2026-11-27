use crate::{
    domain::{bot::Bot, need, strings, DelegationRef, DelegationUpdate, MessageSource},
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

fn message_entry(repos: &Repositories, chat: &Value, bot: &Bot, params: &Value) -> Result<Value> {
    let chat_id = need(chat, "id")?;
    let bot_id = bot.id.as_str();
    let text = need(params, "text")?;
    anyhow::ensure!(
        chat["archived"] != true && strings(&chat["memberIds"]).iter().any(|id| id == bot_id),
        "invalid_args: bot is not a current participant"
    );
    let mut entry = json!({
        "kind":"send-message",
        "author":{"id":bot_id,"name":bot.name},
        "message":{"type":"text","content":text}
    });
    if let Some(parent) = params["replyTo"].as_str() {
        anyhow::ensure!(
            repos.transcript.contains(chat_id, parent)?,
            "invalid_args: reply target is not in this chat"
        );
        entry["replyTo"] = parent.into();
    }
    Ok(entry)
}
pub(crate) fn agent_action(
    store: &Store,
    sessions: &AgentSessions,
    wake_bot: &dyn Fn(&str),
    action: AgentAction,
    params: Value,
) -> Result<Value> {
    let pane = need(&params, "paneId")?;
    let bot = store.tx(|repos, _| repos.bots.get(&repos.bots.require_pane_owner(pane)?))?;
    let bot_id = bot.id.as_str();
    let requested = need(&params, "chatId")?;
    let mut active = sessions.active.lock().unwrap();
    if action == AgentAction::Direct && !active.contains_key(bot_id) {
        let (result, recipients) = store.tx(|repos, events| {
            let dm = repos.chats.open_direct(requested, Some(bot_id), events)?;
            let entry = message_entry(repos, &dm, &bot, &params)?;
            let entry = repos.append_entry(need(&dm, "id")?, entry, events)?;
            let recipients = repos
                .inbox
                .deliver(&dm, &entry, MessageSource::Bot(bot_id), 0)?;
            Ok((json!({"chatId":dm["id"],"entryId":entry["id"]}), recipients))
        })?;
        drop(active);
        for recipient in recipients {
            wake_bot(&recipient);
        }
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
        if let Some(delivery) = &turn.delivery {
            store.tx(|repos, events| {
                repos.finish_delivery(delivery, turn.context.delegation.as_ref(), events)
            })?;
        }
        turn.passed = true;
        return Ok(json!({"passed":true}));
    }
    let (posted_entry, recipients) = store.tx(|repos, events| {
        let chat = repos.chats.get(target)?;
        let mut entry = message_entry(repos, &chat, &bot, &params)?;
        let parent = turn.context.delegation.as_ref();
        let (posted, recipients) = if cross {
            let origin = parent.map_or(turn.chat.as_str(), |link| &link.chat_id);
            let root = parent
                .map(|link| link.root_id.as_str())
                .or(turn.context.message_id.as_deref());
            let mut tracking = entry.clone();
            tracking["replyTo"] = json!(root);
            let destination = repos.chats.get(requested)?;
            tracking["delegation"] =
                json!({"targetChatId":target,"targetName":destination["name"],"state":"queued"});
            if let Some(link) = parent {
                tracking["delegation"]["parentId"] = json!(link.entry_id);
            }
            let tracking = repos.append_entry(origin, tracking, events)?;
            let tracker_id = need(&tracking, "id")?;
            let link = DelegationRef {
                chat_id: origin.into(),
                entry_id: tracker_id.into(),
                root_id: root.unwrap_or(tracker_id).into(),
            };
            let posted = if origin == target {
                tracking
            } else {
                entry["delegatedFrom"] =
                    json!({"chatId":link.chat_id,"entryId":link.entry_id,"rootId":link.root_id});
                repos.append_entry(target, entry, events)?
            };
            let recipients = repos.inbox.deliver(
                &chat,
                &posted,
                MessageSource::Delegation(bot_id),
                turn.depth + 1,
            )?;
            if recipients.is_empty() {
                repos.set_delegation_state(
                    &link,
                    DelegationUpdate::Failed("No recipient for this delegation.".into()),
                    events,
                )?;
            }
            (posted, recipients)
        } else {
            if let Some(link) = parent.filter(|link| link.chat_id == target) {
                entry["delegationId"] = json!(link.entry_id);
                if entry["replyTo"].is_null() {
                    entry["replyTo"] = json!(link.root_id);
                }
            }
            let posted = repos.append_entry(target, entry, events)?;
            let recipients =
                repos
                    .inbox
                    .deliver(&chat, &posted, MessageSource::Bot(bot_id), turn.depth + 1)?;
            if let Some(link) = parent.filter(|link| link.chat_id != target) {
                if repos.transcript.contains(&link.chat_id, &link.entry_id)? {
                    repos.append_entry(&link.chat_id, json!({
                        "kind":"send-message","author":posted["author"],"message":posted["message"],
                        "replyTo":link.root_id,"delegationId":link.entry_id
                    }), events)?;
                }
            }
            (posted, recipients)
        };
        if let Some(delivery) = &turn.delivery {
            repos.finish_delivery(delivery, parent, events)?;
        }
        Ok((posted, recipients))
    })?;
    turn.spoken += 1;
    if cross {
        turn.delegated = true;
    }
    drop(active);
    for recipient in recipients {
        wake_bot(&recipient);
    }
    Ok(json!({"chatId":target,"entryId":posted_entry["id"]}))
}
