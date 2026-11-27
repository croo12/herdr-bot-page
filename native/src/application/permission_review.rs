use crate::{
    domain::need,
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
    wake_bot: &dyn Fn(&str),
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
    turns::agent_action(store, sessions, wake_bot, turns::AgentAction::Say, params)
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

pub(crate) fn refresh_prompts(store: &Store, sessions: &AgentSessions) -> Result<()> {
    let Ok(_guard) = sessions.roster.try_lock() else {
        return Ok(());
    };
    let bots = store.tx(|repos, _| repos.bots.registrations())?;
    if bots.is_empty() {
        return Ok(());
    }
    let agents = sessions.herdr.list_agents()?;
    prompts::refresh_status(store, sessions, &bots, &agents)?;
    reviews::dispatch_reviews(store, sessions, &agents)
}
