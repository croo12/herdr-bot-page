use super::validation::{bot_id, validate};
use crate::{
    domain::{id, need, patch, string_field, ChatJob},
    infrastructure::{
        herdr::{self, Herdr},
        AgentSessions,
    },
    modules::{
        chats::{change, new_chat},
        settings,
    },
    storage::Store,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

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
        bot["permissionMode"] =
            store.tx(|repos, _| settings::defaults(repos))?["permissionMode"].clone();
    }
    validate(
        &mut bot,
        params["allowHomeCwd"] == true || bot_cwd_unchanged(&chat, params),
        false,
    )?;
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
        change(repos, &chat, events)?;
        Ok(json!({"chat":chat}))
    })
}
pub(crate) fn create_bot(
    store: &Store,
    sessions: &AgentSessions,
    enqueue_chat: &dyn Fn(&str, ChatJob),
    mut params: Value,
    quick: bool,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    if let Some(chat) = store.tx(|repos, _| repos.chats.for_request(&params))? {
        return Ok(json!({"chat":chat}));
    }
    let defaults = store.tx(|repos, _| settings::defaults(repos))?;

    if quick && string_field(&params, "name").trim().is_empty() {
        params["name"] = params["provider"].clone();
    }
    need(&params, "name")?;
    let bot_id = bot_id(&params)?;
    if store.tx(|repos, _| Ok(repos.chats.list()?.iter().any(|chat| chat["id"] == bot_id)))? {
        bail!("bot_exists: {bot_id}")
    }
    let mut bot = json!({"provider":params["provider"],"cwd":params["cwd"].as_str().unwrap_or(string_field(&defaults,"cwd")),
        "permissionMode":params["permissionMode"].as_str().unwrap_or(string_field(&defaults,"permissionMode")),"model":params["model"],"reasoningEffort":params["reasoningEffort"],
        "adopted":false,"pendingPrompt":null,"onboarding":null});
    if params["adoptPaneId"].is_null() {
        validate(&mut bot, params["allowHomeCwd"] == true, true)?;
    }
    sessions.herdr.ensure()?;
    let agents = sessions.herdr.agents()?;
    if agents
        .iter()
        .any(|agent| agent["name"] == bot_id && agent["pane_id"] != params["adoptPaneId"])
    {
        bail!("bot_exists: agent {bot_id} already exists")
    }
    let agent = if let Some(pane) = params["adoptPaneId"].as_str() {
        if store.tx(|repos, _| {
            Ok(repos
                .chats
                .list()?
                .iter()
                .any(|chat| chat["bot"]["paneId"] == pane))
        })? {
            bail!("bot_exists: pane already adopted")
        }
        let agent = agents
            .into_iter()
            .find(|agent| agent["pane_id"] == pane)
            .ok_or_else(|| anyhow::anyhow!("unknown_bot: pane {pane}"))?;
        sessions.herdr.run(&["agent", "rename", pane, &bot_id])?;
        bot["adopted"] = true.into();
        bot["provider"] = agent["agent"].clone();
        bot["cwd"] = agent["cwd"].clone();
        agent
    } else {
        sessions.herdr.launch(&bot_id, &bot)?
    };
    bot["paneId"] = agent["pane_id"].clone();
    bot["status"] = agent["agent_status"].clone();
    let mut chat = new_chat(&bot_id, "bot", &params);
    chat["bot"] = bot;
    store.tx(|repos, events| {
        repos.chats.save(&chat)?;
        repos.chats.remember(&params, &chat)?;
        events.push(("chat.listChanged", json!(repos.chats.list()?)));
        Ok(())
    })?;
    if quick {
        enqueue_chat(
            &bot_id,
            ChatJob {
                seed: format!(
                    "Introduce yourself in {} in one sentence.",
                    string_field(&params, "locale")
                ),
                depth: 0,
                exclude: None,
            },
        );
    }
    Ok(json!({"chat":chat}))
}
fn bot_cwd_unchanged(chat: &Value, params: &Value) -> bool {
    params["cwd"].is_null() || params["cwd"] == chat["bot"]["cwd"]
}
