use crate::{
    domain::bot::BotId,
    infrastructure::{
        herdr::{Herdr, HerdrAgent},
        AgentSessions,
    },
    storage::Store,
};
use anyhow::{bail, Context, Result};
use serde_json::Value;

pub(crate) fn connect_bot(store: &Store, sessions: &AgentSessions, id: &BotId) -> Result<Value> {
    let bot_id = id.as_str();
    let _roster = sessions.roster.lock().unwrap();
    if sessions.active.lock().unwrap().contains_key(bot_id)
        || store.tx(|repos, _| repos.reviews.pending_for(bot_id))?
    {
        bail!("bot_not_ready: {bot_id} already has an active turn");
    }
    Ok(connect_for_turn(store, &sessions.herdr, id)?.into_response())
}

/// Called under the roster lock, before the first submission of a new turn.
/// An in-flight turn must keep waiting on the pane that received its prompt.
pub(crate) fn connect_for_turn(store: &Store, herdr: &Herdr, id: &BotId) -> Result<HerdrAgent> {
    let registered = store.tx(|repos, _| repos.bots.registration(id))?;
    let bot = registered.bot;
    let bot_id = bot.id.as_str();
    let agents = match herdr.list_agents() {
        Ok(agents) => agents,
        Err(_) => {
            herdr.ensure()?;
            herdr.list_agents()?
        }
    };
    let mut named = agents.iter().filter(|agent| agent.name() == Some(bot_id));
    let existing = named.next();
    let live = match existing {
        Some(agent) => {
            if named.next().is_some() || agent.provider() != Some(bot.provider.name()) {
                bail!("bot_not_ready: ambiguous or mismatched agent for {bot_id}");
            }
            agent.clone()
        }
        None if registered.session.adopted == Some(false) => {
            let directory = bot
                .working_directory
                .as_deref()
                .filter(|path| !path.as_os_str().to_string_lossy().trim().is_empty())
                .context("invalid_args: missing cwd")?;
            let started = herdr
                .launch(bot_id, directory, &bot.provider)
                .with_context(|| format!("bot_not_ready: could not reconnect {bot_id}"))?;
            // Startup can stop at a provider dialog before Herdr finishes naming the agent.
            if started.name() == Some(bot_id) {
                started
            } else {
                herdr.get_agent(started.required_pane()?)?
            }
        }
        None => bail!("bot_not_ready: adopted agent {bot_id} is offline; reconnect it in Herdr"),
    };
    let pane = live.required_pane()?;
    if live.name() != Some(bot_id) || live.provider() != Some(bot.provider.name()) {
        bail!("bot_not_ready: agent identity changed for {bot_id}");
    }
    if existing.is_none() || registered.session.pane_id.as_deref() != Some(pane) {
        store.tx(|repos, events| repos.rebind_bot_pane(&bot.id, pane, events))?;
    }
    Ok(live)
}
