use crate::{
    domain::{bot::BotId, need},
    modules::bots::RelaunchBotInput,
};
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;

pub(super) fn input(params: &Value) -> Result<RelaunchBotInput> {
    Ok(RelaunchBotInput {
        bot_id: BotId::from(need(params, "botId")?),
        cwd: changed(params, "cwd")?,
        model: changed(params, "model")?,
        reasoning_effort: changed(params, "reasoningEffort")?,
        permission_mode: changed(params, "permissionMode")?,
        allow_home_cwd: params["allowHomeCwd"] == true,
    })
}

// Missing fields keep their saved values. An explicit null clears a setting.
fn changed<T: DeserializeOwned>(params: &Value, field: &str) -> Result<Option<Option<T>>> {
    params
        .get(field)
        .map(Option::<T>::deserialize)
        .transpose()
        .with_context(|| format!("invalid_launch_options: invalid {field}"))
}
