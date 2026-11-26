use crate::{
    domain::{need, string_field},
    infrastructure::{herdr::Herdr, AgentSessions},
    modules::{chats::change, transcript::update_entry},
    storage::Store,
};
use anyhow::{bail, Context, Result};
use serde_json::Value;

pub(crate) fn connect_bot(
    store: &Store,
    sessions: &AgentSessions,
    params: &Value,
) -> Result<Value> {
    let bot_id = need(params, "botId")?;
    let _roster = sessions.roster.lock().unwrap();
    if sessions.active.lock().unwrap().contains_key(bot_id)
        || store.tx(|repos, _| repos.reviews.pending_for(bot_id))?
    {
        bail!("bot_not_ready: {bot_id} already has an active turn");
    }
    connect_for_turn(store, &sessions.herdr, bot_id)
}

/// Called under the roster lock, before the first submission of a new turn.
/// An in-flight turn must keep waiting on the pane that received its prompt.
pub(crate) fn connect_for_turn(store: &Store, herdr: &Herdr, bot_id: &str) -> Result<Value> {
    let chat = store.tx(|repos, _| repos.chats.get(bot_id))?;
    if chat["kind"] != "bot" {
        bail!("unknown_bot: {bot_id}");
    }
    let bot = &chat["bot"];
    let agents = match herdr.agents() {
        Ok(agents) => agents,
        Err(_) => {
            herdr.ensure()?;
            herdr.agents()?
        }
    };
    let mut named = agents.iter().filter(|agent| agent["name"] == bot_id);
    let existing = named.next();
    let live = match existing {
        Some(agent) => {
            if named.next().is_some() || agent["agent"] != bot["provider"] {
                bail!("bot_not_ready: ambiguous or mismatched agent for {bot_id}");
            }
            agent.clone()
        }
        None if bot["adopted"] == false => {
            let started = herdr
                .launch(bot_id, bot)
                .with_context(|| format!("bot_not_ready: could not reconnect {bot_id}"))?;
            // Startup can stop at a provider dialog before Herdr finishes naming the agent.
            if started["name"] == bot_id {
                started
            } else {
                herdr.run(&["agent", "get", need(&started, "pane_id")?])?["agent"].clone()
            }
        }
        None => bail!("bot_not_ready: adopted agent {bot_id} is offline; reconnect it in Herdr"),
    };
    let pane = need(&live, "pane_id")?;
    if live["name"] != bot_id || live["agent"] != bot["provider"] {
        bail!("bot_not_ready: agent identity changed for {bot_id}");
    }
    if existing.is_none() || bot["paneId"] != pane {
        store.tx(|repos, events| {
            for mut row in repos.chats.list()? {
                let owner = string_field(&row, "id");
                if row["kind"] != "bot" || (owner != bot_id && row["bot"]["paneId"] != pane) {
                    continue;
                }
                // Pane numbers can be reused after a session restart. Remove the stale
                // owner's binding in the same commit so CLI replies have exactly one owner.
                if let Some((room, mut entry)) = repos.prompts.pending(owner)? {
                    entry["status"] = "resolved".into();
                    update_entry(repos, &room, &entry, events)?;
                }
                row["bot"]["paneId"] = if owner == bot_id {
                    pane.into()
                } else {
                    Value::Null
                };
                row["bot"]["pendingPrompt"] = Value::Null;
                change(repos, &row, events)?;
            }
            Ok(())
        })?;
    }
    Ok(live)
}
