use super::working_directory::resolve_working_directory;
use crate::{
    domain::{
        bot::{BotDefaults, BotPreferences, PermissionMode, ProviderDraft, SelectionFor},
        id, need, patch, string_field,
    },
    infrastructure::{
        herdr::{self, Herdr},
        AgentSessions,
    },
    storage::Store,
};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) fn load_defaults(store: &Store) -> Result<BotDefaults> {
    store.tx(|repos, _| {
        let preferences: BotPreferences = repos
            .settings
            .get()?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        Ok(BotDefaults::resolve(
            preferences,
            std::env::var("HERDR_BOT_DEFAULT_CWD")
                .ok()
                .map(PathBuf::from),
            std::env::var("HERDR_BOT_DEFAULT_KIND").ok(),
            repos.bots.last_working_directory()?,
        ))
    })
}

pub(crate) fn bot_defaults(store: &Store) -> Result<Value> {
    let defaults = load_defaults(store)?;
    Ok(
        json!({"cwd":defaults.working_directory, "provider":defaults.provider,
        "permissionMode":defaults.permission_mode, "homeDir":dirs::home_dir()}),
    )
}

pub(crate) fn focus_bot(store: &Store, herdr: &Herdr, params: &Value) -> Result<Value> {
    let chat = store.tx(|repos, _| repos.chats.get(need(params, "botId")?))?;
    herdr.focus(need(&chat["bot"], "paneId")?)?;
    Ok(Value::Null)
}
pub(crate) fn list_models(provider: &str) -> Result<Value> {
    Ok(herdr::models(provider).unwrap_or_else(
        |error| json!({"models":[],"source":"unavailable","error":format!("{error:#}")}),
    ))
}
pub(crate) fn adoptable_bots(store: &Store, sessions: &AgentSessions) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let agents = sessions.herdr.ensure_and_list_agents()?;
    let chats = store.tx(|repos, _| repos.chats.list())?;
    Ok(json!(agents.into_iter().filter(|agent|!chats.iter().any(|chat|chat["bot"]["paneId"]==agent["pane_id"])).map(|agent|json!({
            "name":agent["name"],"provider":agent["agent"],"status":agent["agent_status"],"paneId":agent["pane_id"],"cwd":agent["cwd"]})).collect::<Vec<_>>()))
}
pub(crate) fn relaunch_bot(
    store: &Store,
    sessions: &AgentSessions,
    params: &Value,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let bot_id = need(params, "botId")?;
    let mut chat = store.tx(|repos, _| repos.chats.get(bot_id))?;
    if chat["kind"] != "bot" {
        bail!("unknown_bot: {bot_id}")
    }
    if sessions.active.lock().unwrap().contains_key(bot_id) {
        bail!("bot_not_ready: bot is taking a turn")
    }
    let mut bot = chat["bot"].clone();
    patch(
        &mut bot,
        params,
        &["cwd", "permissionMode", "model", "reasoningEffort"],
    );
    if bot["permissionMode"].is_null() {
        bot["permissionMode"] = json!(load_defaults(store)?.permission_mode);
    }
    let provider = ProviderDraft::deserialize(&bot)
        .context("invalid_launch_options: invalid provider settings")?
        .select(SelectionFor::Relaunch, PermissionMode::Ask)?;
    let directory = resolve_working_directory(
        bot["cwd"].as_str().map(Path::new),
        params["allowHomeCwd"] == true || bot_cwd_unchanged(&chat, params),
    )?;
    bot["cwd"] = json!(directory);
    sessions.herdr.ensure()?;
    // Bring up the replacement first. A failed launch leaves the old pane available.
    let temporary = format!("hb-{}", &id()[..8]);
    let agent = sessions.herdr.launch(&temporary, &directory, &provider)?;
    let new_pane = need(&agent, "pane_id")?;
    let old = sessions
        .herdr
        .agents()?
        .into_iter()
        .find(|agent| agent["pane_id"] == chat["bot"]["paneId"] && agent["name"] == bot_id);
    let replaced = (|| -> Result<()> {
        if let Some(agent) = &old {
            sessions
                .herdr
                .run(&["agent", "rename", need(agent, "pane_id")?, "--clear"])?;
        }
        sessions.herdr.run(&["agent", "rename", new_pane, bot_id])?;
        Ok(())
    })();
    if let Err(error) = replaced {
        let _ = sessions.herdr.run(&["pane", "close", new_pane]);
        if let Some(agent) = &old {
            let _ =
                sessions
                    .herdr
                    .run(&["agent", "rename", string_field(agent, "pane_id"), bot_id]);
        }
        return Err(error);
    }
    if let Some(old) = old {
        if bot["adopted"] != true {
            sessions
                .herdr
                .run(&["pane", "close", need(&old, "pane_id")?])?;
        }
    }
    bot["adopted"] = false.into();
    bot["paneId"] = new_pane.into();
    bot["status"] = agent["agent_status"].clone();
    bot["pendingPrompt"] = Value::Null;
    store.tx(|repos, events| {
        chat = repos.chats.get(bot_id)?;
        chat["bot"] = bot;
        repos.chats.save_changed(&chat, events)?;
        Ok(json!({"chat":chat}))
    })
}

fn bot_cwd_unchanged(chat: &Value, params: &Value) -> bool {
    params["cwd"].is_null() || params["cwd"] == chat["bot"]["cwd"]
}
