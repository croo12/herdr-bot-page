use super::lib::load_defaults;
use crate::{
    domain::bot::BotId,
    infrastructure::{
        herdr::{self, Herdr},
        AgentSessions,
    },
    storage::Store,
};
use anyhow::Result;
use serde_json::{json, Value};

pub(crate) fn focus_bot(store: &Store, herdr: &Herdr, bot_id: &BotId) -> Result<()> {
    let registered = store.tx(|repos, _| repos.bots.registration(bot_id))?;
    herdr.focus(registered.session.required_pane()?)
}

pub(crate) fn list_models(provider: &str) -> Result<Value> {
    Ok(herdr::models(provider).unwrap_or_else(
        |error| json!({"models":[],"source":"unavailable","error":format!("{error:#}")}),
    ))
}

pub(crate) fn adoptable_bots(store: &Store, sessions: &AgentSessions) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let agents = sessions.herdr.ensure_and_list_agents()?;
    let bound = store.tx(|repos, _| repos.bots.bound_panes())?;
    let available = agents
        .into_iter()
        .filter(|agent| agent.pane_id().is_some_and(|pane| !bound.contains(pane)))
        .map(|agent| {
            json!({
                "name": agent.name(), "provider": agent.provider(), "status": agent.status(),
                "paneId": agent.pane_id(), "cwd": agent.cwd(),
            })
        })
        .collect::<Vec<_>>();
    Ok(Value::Array(available))
}

pub(crate) fn bot_defaults(store: &Store) -> Result<Value> {
    let defaults = load_defaults(store)?;
    Ok(
        json!({"cwd":defaults.working_directory, "provider":defaults.provider,
        "permissionMode":defaults.permission_mode, "homeDir":dirs::home_dir()}),
    )
}
