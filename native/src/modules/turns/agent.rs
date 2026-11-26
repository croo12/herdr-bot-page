use crate::{
    domain::{need, strings, ChatJob},
    infrastructure::AgentSessions,
    modules::{
        chats::{self, ChatEdit},
        transcript::{append, notice},
    },
    storage::Store,
};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
#[derive(PartialEq, Eq)]
pub(crate) enum AgentAction {
    Say,
    Pass,
    Delegate,
    Profile,
    UpdateProfile,
}
pub(crate) fn agent_action(
    store: &Store,
    sessions: &AgentSessions,
    enqueue_chat: &dyn Fn(&str, ChatJob),
    action: AgentAction,
    params: Value,
) -> Result<Value> {
    let pane = need(&params, "paneId")?;
    let bot = store
        .tx(|repos, _| repos.chats.list())?
        .into_iter()
        .find(|chat| chat["bot"]["paneId"] == pane)
        .ok_or_else(|| anyhow!("unknown_bot: pane {pane}"))?;
    let bot_id = need(&bot, "id")?;
    if action == AgentAction::Profile {
        return Ok(bot);
    }
    if action == AgentAction::UpdateProfile {
        let mut patch = params;
        patch["chatId"] = bot_id.into();
        return chats::edit_chat(store, ChatEdit::Profile, &patch);
    }
    let target = need(&params, "chatId")?;
    let target_chat = store.tx(|repos, _| repos.chats.get(target))?;
    let mut active = sessions.active.lock().unwrap();
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
    let text = need(&params, "text")?;
    let posted_entry = store.tx(|repos, events| {
        let mut entry = json!({"kind":"send-message","message":{"type":"text","content":text},"author":{"id":bot_id,"name":bot["name"]}});
        if let Some(parent) = params["replyTo"].as_str() {
            if !repos.transcript.contains(target, parent)? {
                bail!("invalid_args: reply target is not in this chat");
            }
            entry["replyTo"] = parent.into();
        }
        append(repos, target, entry, events)
    })?;
    turn.spoken += 1;
    if cross {
        turn.delegated = true;
        let depth = turn.depth + 1;
        let from = turn.chat.clone();
        drop(active);
        notice(
            store,
            &from,
            &format!("{bot_id} sent a message to {target}."),
        );
        enqueue_chat(
            target,
            ChatJob {
                seed: text.into(),
                depth,
                exclude: Some(bot_id.into()),
            },
        );
    }
    Ok(json!({"entryId":posted_entry["id"]}))
}
