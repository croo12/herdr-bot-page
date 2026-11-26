use crate::{
    domain::{need, ChatJob},
    infrastructure::AgentSessions,
    modules::{prompts, reviews, turns},
    storage::Store,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub(crate) fn post_reply(
    store: &Store,
    sessions: &AgentSessions,
    user: &str,
    enqueue_chat: &dyn Fn(&str, ChatJob),
    params: Value,
) -> Result<Value> {
    let has_decision = !params["decision"].is_null();
    if params["replyTo"].is_null() {
        if has_decision {
            bail!("invalid_args: reply to a permission request");
        }
    } else {
        let ticket = need(&params, "replyTo")?;
        if reviews::has_ticket(store, ticket)? || has_decision {
            return apply_review(store, sessions, user, &params);
        }
    }
    turns::agent_action(
        store,
        sessions,
        enqueue_chat,
        turns::AgentAction::Say,
        params,
    )
}

fn apply_review(
    store: &Store,
    sessions: &AgentSessions,
    user: &str,
    params: &Value,
) -> Result<Value> {
    // Keep the reviewer, room membership and pending prompt stable through the answer.
    let _guard = sessions.roster.lock().unwrap();
    let decision = params["decision"].as_str().unwrap_or("human");
    if let Some(permission) = reviews::resolve_reply(store, &sessions.herdr, params, decision)? {
        let answer = json!({
            "botId": permission.bot_id, "signature": permission.signature,
            "action": if decision == "approve" { "option" } else { "cancel" },
            "key": permission.once_key,
        });
        prompts::answer_locked(store, sessions, user, &answer, Some(&permission.audit))?;
    }
    Ok(json!({"decision":decision}))
}
