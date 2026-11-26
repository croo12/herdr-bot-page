use super::validation::{bot_id, validate};
use crate::modules::chats::new_chat;
use crate::modules::{settings, turns};
use crate::{
    domain::{id, need, patch, string_field},
    infrastructure::herdr,
    modules::{chats::change, turns::Job},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) fn focus_bot(engine: &Engine, params: &Value) -> Result<Value> {
    let chat = engine.tx(|repos, _| repos.chats.get(need(params, "botId")?))?;
    engine.herdr.focus(need(&chat["bot"], "paneId")?)?;
    Ok(Value::Null)
}
pub(crate) fn list_models(provider: &str) -> Result<Value> {
    Ok(herdr::models(provider).unwrap_or_else(
        |error| json!({"models":[],"source":"unavailable","error":format!("{error:#}")}),
    ))
}
pub(crate) fn adoptable_bots(engine: &Engine) -> Result<Value> {
    let _guard = engine.roster.lock().unwrap();
    engine.herdr.ensure()?;
    let agents = engine.herdr.agents()?;
    let chats = engine.tx(|repos, _| repos.chats.list())?;
    Ok(json!(agents.into_iter().filter(|agent|!chats.iter().any(|chat|chat["bot"]["paneId"]==agent["pane_id"])).map(|agent|json!({
            "name":agent["name"],"provider":agent["agent"],"status":agent["agent_status"],"paneId":agent["pane_id"],"cwd":agent["cwd"]})).collect::<Vec<_>>()))
}
pub(crate) fn relaunch_bot(engine: &Engine, params: &Value) -> Result<Value> {
    let _guard = engine.roster.lock().unwrap();
    let bot_id = need(params, "botId")?;
    let mut chat = engine.tx(|repos, _| repos.chats.get(bot_id))?;
    if chat["kind"] != "bot" {
        bail!("unknown_bot: {bot_id}")
    }
    if engine.active.lock().unwrap().contains_key(bot_id) {
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
            engine.tx(|repos, _| settings::defaults(repos))?["permissionMode"].clone();
    }
    validate(
        &mut bot,
        params["allowHomeCwd"] == true || bot_cwd_unchanged(&chat, params),
        false,
    )?;
    engine.herdr.ensure()?;
    // Bring up the replacement first. A failed launch leaves the old pane available.
    let temporary = format!("hb-{}", &id()[..8]);
    let agent = engine.herdr.launch(&temporary, &bot)?;
    let new_pane = need(&agent, "pane_id")?;
    let old = engine
        .herdr
        .agents()?
        .into_iter()
        .find(|agent| agent["pane_id"] == chat["bot"]["paneId"] && agent["name"] == bot_id);
    let replaced = (|| -> Result<()> {
        if let Some(agent) = &old {
            engine
                .herdr
                .run(&["agent", "rename", need(agent, "pane_id")?, "--clear"])?;
        }
        engine.herdr.run(&["agent", "rename", new_pane, bot_id])?;
        Ok(())
    })();
    if let Err(error) = replaced {
        let _ = engine.herdr.run(&["pane", "close", new_pane]);
        if let Some(agent) = &old {
            let _ = engine
                .herdr
                .run(&["agent", "rename", string_field(agent, "pane_id"), bot_id]);
        }
        return Err(error);
    }
    if let Some(old) = old {
        if bot["adopted"] != true {
            engine
                .herdr
                .run(&["pane", "close", need(&old, "pane_id")?])?;
        }
    }
    bot["adopted"] = false.into();
    bot["paneId"] = new_pane.into();
    bot["status"] = agent["agent_status"].clone();
    bot["pendingPrompt"] = Value::Null;
    engine.tx(|repos, events| {
        chat = repos.chats.get(bot_id)?;
        chat["bot"] = bot;
        change(repos, &chat, events)?;
        Ok(json!({"chat":chat}))
    })
}
pub(crate) fn create_bot(engine: &Arc<Engine>, mut params: Value, quick: bool) -> Result<Value> {
    let _guard = engine.roster.lock().unwrap();
    if let Some(chat) = engine.tx(|repos, _| repos.chats.for_request(&params))? {
        return Ok(json!({"chat":chat}));
    }
    let defaults = engine.tx(|repos, _| settings::defaults(repos))?;

    if quick && string_field(&params, "name").trim().is_empty() {
        params["name"] = params["provider"].clone();
    }
    need(&params, "name")?;
    let bot_id = bot_id(&params)?;
    if engine.tx(|repos, _| Ok(repos.chats.list()?.iter().any(|chat| chat["id"] == bot_id)))? {
        bail!("bot_exists: {bot_id}")
    }
    let mut bot = json!({"provider":params["provider"],"cwd":params["cwd"].as_str().unwrap_or(string_field(&defaults,"cwd")),
        "permissionMode":params["permissionMode"].as_str().unwrap_or(string_field(&defaults,"permissionMode")),"model":params["model"],"reasoningEffort":params["reasoningEffort"],
        "adopted":false,"pendingPrompt":null,"onboarding":null});
    if params["adoptPaneId"].is_null() {
        validate(&mut bot, params["allowHomeCwd"] == true, true)?;
    }
    engine.herdr.ensure()?;
    let agents = engine.herdr.agents()?;
    if agents
        .iter()
        .any(|agent| agent["name"] == bot_id && agent["pane_id"] != params["adoptPaneId"])
    {
        bail!("bot_exists: agent {bot_id} already exists")
    }
    let agent = if let Some(pane) = params["adoptPaneId"].as_str() {
        if engine.tx(|repos, _| {
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
        engine.herdr.run(&["agent", "rename", pane, &bot_id])?;
        bot["adopted"] = true.into();
        bot["provider"] = agent["agent"].clone();
        bot["cwd"] = agent["cwd"].clone();
        agent
    } else {
        engine.herdr.launch(&bot_id, &bot)?
    };
    bot["paneId"] = agent["pane_id"].clone();
    bot["status"] = agent["agent_status"].clone();
    let mut chat = new_chat(&bot_id, "bot", &params);
    chat["bot"] = bot;
    engine.tx(|repos, events| {
        repos.chats.save(&chat)?;
        repos.chats.remember(&params, &chat)?;
        events.push(("chat.listChanged", json!(repos.chats.list()?)));
        Ok(())
    })?;
    if quick {
        turns::enqueue(
            engine,
            &bot_id,
            Job {
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
