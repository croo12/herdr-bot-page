use super::validation::{bot_id, validate};
use crate::modules::chats::new_chat;
use crate::modules::{settings, turns};
use crate::{
    domain::{id, need, patch, s},
    infrastructure::herdr,
    modules::{chats::change, turns::Job},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};
use std::sync::Arc;

pub(crate) fn focus_bot(engine: &Engine, p: &V) -> Result<V> {
    let v = engine.tx(|c, _| c.chats.get(need(p, "botId")?))?;
    engine.herdr.focus(need(&v["bot"], "paneId")?)?;
    Ok(V::Null)
}
pub(crate) fn list_models(provider: &str) -> Result<V> {
    Ok(herdr::models(provider)
        .unwrap_or_else(|e| json!({"models":[],"source":"unavailable","error":format!("{e:#}")})))
}
pub(crate) fn adoptable_bots(engine: &Engine) -> Result<V> {
    let _guard = engine.roster.lock().unwrap();
    engine.herdr.ensure()?;
    let agents = engine.herdr.agents()?;
    let chats = engine.tx(|c, _| c.chats.list())?;
    Ok(json!(agents.into_iter().filter(|a|!chats.iter().any(|c|c["bot"]["paneId"]==a["pane_id"])).map(|a|json!({
            "name":a["name"],"provider":a["agent"],"status":a["agent_status"],"paneId":a["pane_id"],"cwd":a["cwd"]})).collect::<Vec<_>>()))
}
pub(crate) fn relaunch_bot(engine: &Engine, p: &V) -> Result<V> {
    let _guard = engine.roster.lock().unwrap();
    let bot_id = need(p, "botId")?;
    let mut v = engine.tx(|c, _| c.chats.get(bot_id))?;
    if v["kind"] != "bot" {
        bail!("unknown_bot: {bot_id}")
    }
    if engine.active.lock().unwrap().contains_key(bot_id) {
        bail!("bot_not_ready: bot is taking a turn")
    }
    let mut bot = v["bot"].clone();
    patch(
        &mut bot,
        p,
        &["cwd", "permissionMode", "model", "reasoningEffort"],
    );
    if bot["permissionMode"].is_null() {
        bot["permissionMode"] = engine.tx(|c, _| settings::defaults(c))?["permissionMode"].clone();
    }
    validate(
        &mut bot,
        p["allowHomeCwd"] == true || bot_cwd_unchanged(&v, p),
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
        .find(|a| a["pane_id"] == v["bot"]["paneId"] && a["name"] == bot_id);
    let replaced = (|| -> Result<()> {
        if let Some(a) = &old {
            engine
                .herdr
                .run(&["agent", "rename", need(a, "pane_id")?, "--clear"])?;
        }
        engine.herdr.run(&["agent", "rename", new_pane, bot_id])?;
        Ok(())
    })();
    if let Err(e) = replaced {
        let _ = engine.herdr.run(&["pane", "close", new_pane]);
        if let Some(a) = &old {
            let _ = engine
                .herdr
                .run(&["agent", "rename", s(a, "pane_id"), bot_id]);
        }
        return Err(e);
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
    bot["pendingPrompt"] = V::Null;
    engine.tx(|c, ev| {
        v = c.chats.get(bot_id)?;
        v["bot"] = bot;
        change(c, &v, ev)?;
        Ok(json!({"chat":v}))
    })
}
pub(crate) fn create_bot(engine: &Arc<Engine>, mut p: V, quick: bool) -> Result<V> {
    let _guard = engine.roster.lock().unwrap();
    if let Some(v) = engine.tx(|c, _| c.chats.for_request(&p))? {
        return Ok(json!({"chat":v}));
    }
    let defaults = engine.tx(|c, _| settings::defaults(c))?;

    if quick && s(&p, "name").trim().is_empty() {
        p["name"] = p["provider"].clone();
    }
    need(&p, "name")?;
    let bot_id = bot_id(&p)?;
    if engine.tx(|c, _| Ok(c.chats.list()?.iter().any(|x| x["id"] == bot_id)))? {
        bail!("bot_exists: {bot_id}")
    }
    let mut bot = json!({"provider":p["provider"],"cwd":p["cwd"].as_str().unwrap_or(s(&defaults,"cwd")),
        "permissionMode":p["permissionMode"].as_str().unwrap_or(s(&defaults,"permissionMode")),"model":p["model"],"reasoningEffort":p["reasoningEffort"],
        "adopted":false,"pendingPrompt":null,"onboarding":null});
    if p["adoptPaneId"].is_null() {
        validate(&mut bot, p["allowHomeCwd"] == true, true)?;
    }
    engine.herdr.ensure()?;
    let agents = engine.herdr.agents()?;
    if agents
        .iter()
        .any(|a| a["name"] == bot_id && a["pane_id"] != p["adoptPaneId"])
    {
        bail!("bot_exists: agent {bot_id} already exists")
    }
    let a = if let Some(pane) = p["adoptPaneId"].as_str() {
        if engine.tx(|c, _| Ok(c.chats.list()?.iter().any(|x| x["bot"]["paneId"] == pane)))? {
            bail!("bot_exists: pane already adopted")
        }
        let agent = agents
            .into_iter()
            .find(|a| a["pane_id"] == pane)
            .ok_or_else(|| anyhow::anyhow!("unknown_bot: pane {pane}"))?;
        engine.herdr.run(&["agent", "rename", pane, &bot_id])?;
        bot["adopted"] = true.into();
        bot["provider"] = agent["agent"].clone();
        bot["cwd"] = agent["cwd"].clone();
        agent
    } else {
        engine.herdr.launch(&bot_id, &bot)?
    };
    bot["paneId"] = a["pane_id"].clone();
    bot["status"] = a["agent_status"].clone();
    let mut v = new_chat(&bot_id, "bot", &p);
    v["bot"] = bot;
    engine.tx(|c, ev| {
        c.chats.save(&v)?;
        c.chats.remember(&p, &v)?;
        ev.push(("chat.listChanged", json!(c.chats.list()?)));
        Ok(())
    })?;
    if quick {
        turns::enqueue(
            engine,
            &bot_id,
            Job {
                seed: format!("Introduce yourself in {} in one sentence.", s(&p, "locale")),
                depth: 0,
                exclude: None,
            },
        );
    }
    Ok(json!({"chat":v}))
}
fn bot_cwd_unchanged(v: &V, p: &V) -> bool {
    p["cwd"].is_null() || p["cwd"] == v["bot"]["cwd"]
}
