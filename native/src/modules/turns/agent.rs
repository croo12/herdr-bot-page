use crate::modules::{chats, reviews};
use crate::{
    domain::{need, strings},
    modules::{chats::ChatEdit, transcript::append, turns::Job},
    Engine,
};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value as V};
use std::sync::Arc;
#[derive(PartialEq, Eq)]
pub(crate) enum AgentAction {
    Say,
    Pass,
    Delegate,
    Profile,
    UpdateProfile,
}
pub(crate) fn say(engine: &Arc<Engine>, p: V) -> Result<V> {
    if !p["replyTo"].is_null() {
        let assigned = engine.tx(|c, _| c.reviews.ticket_exists(need(&p, "replyTo")?))?;
        if assigned || !p["decision"].is_null() {
            return reviews::review_reply(engine, &p);
        }
    } else if !p["decision"].is_null() {
        bail!("invalid_args: reply to a permission request")
    }
    agent_action(engine, AgentAction::Say, p)
}
pub(crate) fn agent_action(engine: &Arc<Engine>, action: AgentAction, p: V) -> Result<V> {
    let pane = need(&p, "paneId")?;
    let bot = engine
        .tx(|c, _| c.chats.list())?
        .into_iter()
        .find(|x| x["bot"]["paneId"] == pane)
        .ok_or_else(|| anyhow!("unknown_bot: pane {pane}"))?;
    let bot_id = need(&bot, "id")?;
    if action == AgentAction::Profile {
        return Ok(bot);
    }
    if action == AgentAction::UpdateProfile {
        let mut patch = p;
        patch["chatId"] = bot_id.into();
        return chats::edit_chat(engine, ChatEdit::Profile, &patch);
    }
    let target = need(&p, "chatId")?;
    let target_chat = engine.tx(|c, _| c.chats.get(target))?;
    let mut active = engine.active.lock().unwrap();
    let turn = active
        .get_mut(bot_id)
        .ok_or_else(|| anyhow!("bot_not_ready: no open turn for {bot_id}"))?;
    let cross = action == AgentAction::Delegate;
    if !cross && target != turn.chat {
        bail!("invalid_args: wrong chat for this turn")
    }
    if cross
        && (turn.depth >= 2
            || turn.delegated
            || target == turn.chat
            || target == bot_id
            || (target_chat["kind"] == "room"
                && !strings(&target_chat["memberIds"]).contains(&bot_id.to_owned())))
    {
        bail!("invalid_args: delegation is not available for this target or turn")
    }
    if turn.passed || turn.spoken >= 2 {
        bail!("bot_not_ready: turn has ended")
    }
    if action == AgentAction::Pass {
        turn.passed = true;
        return Ok(json!({"passed":true}));
    }
    let text = need(&p, "text")?;
    let e = engine.tx(|c, ev| {
        let mut entry = json!({"kind":"send-message","message":{"type":"text","content":text},"author":{"id":bot_id,"name":bot["name"]}});
        if let Some(parent) = p["replyTo"].as_str() {
            if !c.transcript.contains(target, parent)? {
                bail!("invalid_args: reply target is not in this chat");
            }
            entry["replyTo"] = parent.into();
        }
        append(c, target, entry, ev)
    })?;
    turn.spoken += 1;
    if cross {
        turn.delegated = true;
        let depth = turn.depth + 1;
        let from = turn.chat.clone();
        drop(active);
        super::service::notice(
            engine,
            &from,
            &format!("{bot_id} sent a message to {target}."),
        );
        super::service::enqueue(
            engine,
            target,
            Job {
                seed: text.into(),
                depth,
                exclude: Some(bot_id.into()),
            },
        );
    }
    Ok(json!({"entryId":e["id"]}))
}
