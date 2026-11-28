use crate::{infrastructure::AgentSessions, storage::Store};
use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdoptionCandidate {
    pane_id: String,
    name: Option<String>,
    provider: Option<String>,
    status: Option<String>,
    cwd: Option<String>,
}

pub(crate) fn get_adoption_candidates(
    store: &Store,
    sessions: &AgentSessions,
) -> Result<Vec<AdoptionCandidate>> {
    let _guard = sessions.roster.lock().unwrap();
    let agents = sessions.herdr.ensure_and_list_agents()?;
    let bound = store.tx(|repos, _| repos.bots.bound_panes())?;
    let candidates = agents.iter().filter_map(|agent| {
        let pane_id = agent.pane_id().filter(|pane| !bound.contains(*pane))?;
        Some(AdoptionCandidate {
            pane_id: pane_id.to_owned(),
            name: agent.name().map(str::to_owned),
            provider: agent.provider().map(str::to_owned),
            status: agent.status().map(str::to_owned),
            cwd: agent.cwd().map(str::to_owned),
        })
    });
    Ok(candidates.collect())
}
