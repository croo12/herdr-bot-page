use super::lib::{register_bot, CreateBotInput, Registration};
use crate::{infrastructure::AgentSessions, storage::Store};
use anyhow::Result;
use serde_json::{json, Value};

pub(crate) fn create_bot(
    store: &Store,
    sessions: &AgentSessions,
    input: CreateBotInput,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let profile = match register_bot(store, &sessions.herdr, input)? {
        Registration::Created(_, profile) | Registration::AlreadyCreated(profile) => profile,
    };
    Ok(json!({"chat": profile}))
}
