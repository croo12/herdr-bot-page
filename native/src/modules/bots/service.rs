use super::validation::validate;
use crate::{
    domain::{
        bot::{Bot, BotDefaults, BotDraft, BotPreferences, LaunchSettings},
        id, need, new_chat, patch, string_field, ChatJob,
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
use std::path::PathBuf;

fn load_defaults(store: &Store) -> Result<BotDefaults> {
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
    sessions.herdr.ensure()?;
    let agents = sessions.herdr.agents()?;
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
    let mut settings = LaunchSettings::deserialize(&bot)
        .context("invalid_launch_options: invalid launch settings")?;
    validate(
        &mut settings,
        params["allowHomeCwd"] == true || bot_cwd_unchanged(&chat, params),
        false,
    )?;
    bot["cwd"] = json!(settings.cwd);
    sessions.herdr.ensure()?;
    // Bring up the replacement first. A failed launch leaves the old pane available.
    let temporary = format!("hb-{}", &id()[..8]);
    let agent = sessions.herdr.launch(&temporary, &bot)?;
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
pub(crate) fn create_bot(
    store: &Store,
    sessions: &AgentSessions,
    enqueue_greeting: &dyn Fn(&str, &str, ChatJob),
    mut params: Value,
    quick: bool,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    if let Some(chat) = store.tx(|repos, _| repos.chats.for_request(&params))? {
        return Ok(json!({"chat":chat}));
    }
    let list_agents = || {
        sessions.herdr.ensure()?;
        sessions.herdr.agents()
    };
    let read_draft = |input: &Value| -> Result<_> {
        let draft = BotDraft::deserialize(input).context("invalid_args: invalid bot settings")?;
        let settings = draft.launch_settings(&load_defaults(store)?);
        Ok((draft, settings))
    };
    let agents;
    let (draft, settings, adopted) = if let Some(pane) = params["adoptPaneId"].as_str() {
        if store.tx(|repos, _| Ok(repos.chats.pane_owner(pane)?.is_some()))? {
            bail!("bot_exists: pane already adopted")
        }
        agents = list_agents()?;
        let agent = agents
            .iter()
            .find(|agent| agent["pane_id"] == pane)
            .ok_or_else(|| anyhow::anyhow!("unknown_bot: pane {pane}"))?;
        params["provider"] = need(agent, "agent")?.into();
        let (draft, mut settings) = read_draft(&params)?;
        settings.cwd = agent["cwd"].as_str().map(PathBuf::from);
        (draft, settings, Some(agent))
    } else {
        need(&params, "provider")?;
        let (draft, mut settings) = read_draft(&params)?;
        validate(&mut settings, params["allowHomeCwd"] == true, true)?;
        agents = list_agents()?;
        (draft, settings, None)
    };
    let bot = Bot::new(draft, &settings, quick)?;
    let bot_id = bot.id.as_str();
    if store.tx(|repos, _| Ok(repos.chats.find(bot_id)?.is_some()))? {
        bail!("bot_exists: {bot_id}")
    }
    if agents
        .iter()
        .any(|agent| agent["name"] == bot_id && agent["pane_id"] != params["adoptPaneId"])
    {
        bail!("bot_exists: agent {bot_id} already exists")
    }
    let mut facts = serde_json::to_value(&settings)?;
    let agent = if let Some(agent) = adopted {
        sessions
            .herdr
            .run(&["agent", "rename", need(agent, "pane_id")?, bot_id])?;
        agent.clone()
    } else {
        sessions.herdr.launch(bot_id, &facts)?
    };
    facts["adopted"] = adopted.is_some().into();
    facts["paneId"] = agent["pane_id"].clone();
    facts["status"] = agent["agent_status"].clone();
    facts["pendingPrompt"] = Value::Null;
    facts["onboarding"] = Value::Null;
    let mut chat = new_chat(
        bot_id,
        "bot",
        &json!({
            "name":bot.name, "title":bot.label, "description":bot.instruction,
            "avatarColor":bot.avatar_color, "avatarShape":params["avatarShape"]
        }),
    );
    chat["bot"] = facts;
    store.tx(|repos, events| {
        repos.chats.save(&chat)?;
        repos.chats.remember(&params, &chat)?;
        events.push(("chat.listChanged", json!(repos.chats.visible_list()?)));
        Ok(())
    })?;
    if quick {
        let dm = store.tx(|repos, events| repos.chats.open_direct(bot_id, None, events))?;
        enqueue_greeting(
            bot_id,
            need(&dm, "id")?,
            ChatJob {
                delivery: None,
                context: Default::default(),
                seed: format!(
                    "Introduce yourself in {} in one sentence.",
                    string_field(&params, "locale")
                ),
                depth: 0,
            },
        );
    }
    Ok(json!({"chat":chat}))
}
fn bot_cwd_unchanged(chat: &Value, params: &Value) -> bool {
    params["cwd"].is_null() || params["cwd"] == chat["bot"]["cwd"]
}
