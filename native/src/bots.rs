use crate::{
    api::{expand, remember, request_chat},
    store::*,
    turns::Job,
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};
use std::sync::Arc;

const KINDS: &[&str] = &[
    "pi",
    "claude",
    "codex",
    "gemini",
    "cursor",
    "devin",
    "agy",
    "cline",
    "omp",
    "mastracode",
    "opencode",
    "copilot",
    "kimi",
    "kiro",
    "droid",
    "amp",
    "grok",
    "hermes",
    "kilo",
    "qodercli",
    "qwen",
    "maki",
    "muse",
];
fn bot_id(p: &V) -> Result<String> {
    if let Some(id) = p["id"].as_str() {
        if !id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || id.len() > 32
            || id.starts_with("room-")
            || !id
                .bytes()
                .all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == b'-' || x == b'_')
        {
            bail!("invalid_bot_id: {id}")
        }
        return Ok(id.into());
    }
    let slug: String = s(p, "name")
        .to_lowercase()
        .chars()
        .map(|x| if x.is_ascii_alphanumeric() { x } else { '-' })
        .take(20)
        .collect();
    let slug = slug.trim_matches('-');
    let prefix = if slug.starts_with(|x: char| x.is_ascii_lowercase()) && !slug.starts_with("room-")
    {
        slug
    } else {
        "bot"
    };
    Ok(format!("{prefix}-{}", &id()[..6]))
}
fn validate(bot: &mut V, allow_home: bool, require_choices: bool) -> Result<()> {
    let kind = need(bot, "provider")?;
    if !KINDS.contains(&kind) {
        bail!("unsupported_kind: {kind}")
    }
    if !matches!(s(bot, "permissionMode"), "ask" | "auto") {
        bail!("invalid_launch_options: invalid permission mode")
    }
    let reasoning = matches!(kind, "codex" | "claude" | "grok");
    let model = reasoning || matches!(kind, "gemini" | "agy" | "opencode");
    for (field, can) in [("model", model), ("reasoningEffort", reasoning)] {
        let selected = !s(bot, field).trim().is_empty();
        if (selected && !can) || (require_choices && can && !selected) {
            bail!("invalid_launch_options: check {field} for {kind}")
        }
    }
    if bot["reasoningEffort"].is_string()
        && !["low", "medium", "high", "xhigh", "max"].contains(&s(bot, "reasoningEffort"))
    {
        bail!("invalid_launch_options: unknown reasoning effort")
    }
    if s(bot, "cwd").is_empty() {
        bail!("missing_cwd: choose a working folder in Settings or Advanced setup")
    }
    let cwd = expand(s(bot, "cwd"));
    if !cwd.is_dir() {
        bail!("missing_cwd: {}", cwd.display())
    }
    if !allow_home
        && cwd.canonicalize().ok() == dirs::home_dir().and_then(|p| p.canonicalize().ok())
    {
        bail!("home_cwd_requires_confirmation: confirm the home folder")
    }
    bot["cwd"] = json!(cwd);
    Ok(())
}
impl Engine {
    pub(crate) fn roster_call(self: &Arc<Self>, method: &str, mut p: V) -> Result<V> {
        let _roster = self.roster.lock().unwrap();
        if method == "chat.delete" {
            return self.delete_chats(&p);
        }
        if method == "bot.listAdoptable" {
            self.herdr.ensure()?;
            let agents = self.herdr.agents()?;
            let chats = self.tx(|c, _| list(c))?;
            return Ok(json!(agents.into_iter().filter(|a|!chats.iter().any(|c|c["bot"]["paneId"]==a["pane_id"])).map(|a|json!({
                "name":a["name"],"provider":a["agent"],"status":a["agent_status"],"paneId":a["pane_id"],"cwd":a["cwd"]})).collect::<Vec<_>>()));
        }
        if method == "bot.retrySetup" || method == "bot.updateLaunchSettings" {
            let bot_id = need(&p, "botId")?;
            let mut v = self.tx(|c, _| chat(c, bot_id))?;
            if v["kind"] != "bot" {
                bail!("unknown_bot: {bot_id}")
            }
            if self.active.lock().unwrap().contains_key(bot_id) {
                bail!("bot_not_ready: bot is taking a turn")
            }
            let mut bot = v["bot"].clone();
            patch(
                &mut bot,
                &p,
                &["cwd", "permissionMode", "model", "reasoningEffort"],
            );
            if bot["permissionMode"].is_null() {
                bot["permissionMode"] = self.tx(|c, _| self.defaults(c))?["permissionMode"].clone();
            }
            validate(
                &mut bot,
                p["allowHomeCwd"] == true || bot_cwd_unchanged(&v, &p),
                false,
            )?;
            self.herdr.ensure()?;
            // Bring up the replacement first. A failed launch leaves the old pane available.
            let temporary = format!("hb-{}", &id()[..8]);
            let agent = self.herdr.launch(&temporary, &bot)?;
            let new_pane = need(&agent, "pane_id")?;
            let old = self
                .herdr
                .agents()?
                .into_iter()
                .find(|a| a["pane_id"] == v["bot"]["paneId"] && a["name"] == bot_id);
            let replaced = (|| -> Result<()> {
                if let Some(a) = &old {
                    self.herdr
                        .run(&["agent", "rename", need(a, "pane_id")?, "--clear"])?;
                }
                self.herdr.run(&["agent", "rename", new_pane, bot_id])?;
                Ok(())
            })();
            if let Err(e) = replaced {
                let _ = self.herdr.run(&["pane", "close", new_pane]);
                if let Some(a) = &old {
                    let _ = self
                        .herdr
                        .run(&["agent", "rename", s(a, "pane_id"), bot_id]);
                }
                return Err(e);
            }
            if let Some(old) = old {
                if bot["adopted"] != true {
                    self.herdr.run(&["pane", "close", need(&old, "pane_id")?])?;
                }
            }
            bot["adopted"] = false.into();
            bot["paneId"] = new_pane.into();
            bot["status"] = agent["agent_status"].clone();
            bot["pendingPrompt"] = V::Null;
            return self.tx(|c, ev| {
                v = chat(c, bot_id)?;
                v["bot"] = bot;
                change(c, &v, ev)?;
                Ok(json!({"chat":v}))
            });
        }
        if let Some(v) = self.tx(|c, _| request_chat(c, &p))? {
            return Ok(json!({"chat":v}));
        }
        let defaults = self.tx(|c, _| self.defaults(c))?;
        let quick = method == "bot.quickCreate";
        if quick && s(&p, "name").trim().is_empty() {
            p["name"] = p["provider"].clone();
        }
        need(&p, "name")?;
        let bot_id = bot_id(&p)?;
        if self.tx(|c, _| Ok(list(c)?.iter().any(|x| x["id"] == bot_id)))? {
            bail!("bot_exists: {bot_id}")
        }
        let mut bot = json!({"provider":p["provider"],"cwd":p["cwd"].as_str().unwrap_or(s(&defaults,"cwd")),
            "permissionMode":p["permissionMode"].as_str().unwrap_or(s(&defaults,"permissionMode")),"model":p["model"],"reasoningEffort":p["reasoningEffort"],
            "adopted":false,"pendingPrompt":null,"onboarding":null});
        if p["adoptPaneId"].is_null() {
            validate(&mut bot, p["allowHomeCwd"] == true, true)?;
        }
        self.herdr.ensure()?;
        let agents = self.herdr.agents()?;
        if agents
            .iter()
            .any(|a| a["name"] == bot_id && a["pane_id"] != p["adoptPaneId"])
        {
            bail!("bot_exists: agent {bot_id} already exists")
        }
        let a = if let Some(pane) = p["adoptPaneId"].as_str() {
            if self.tx(|c, _| Ok(list(c)?.iter().any(|x| x["bot"]["paneId"] == pane)))? {
                bail!("bot_exists: pane already adopted")
            }
            let agent = agents
                .into_iter()
                .find(|a| a["pane_id"] == pane)
                .ok_or_else(|| anyhow::anyhow!("unknown_bot: pane {pane}"))?;
            self.herdr.run(&["agent", "rename", pane, &bot_id])?;
            bot["adopted"] = true.into();
            bot["provider"] = agent["agent"].clone();
            bot["cwd"] = agent["cwd"].clone();
            agent
        } else {
            self.herdr.launch(&bot_id, &bot)?
        };
        bot["paneId"] = a["pane_id"].clone();
        bot["status"] = a["agent_status"].clone();
        let mut v = new_chat(&bot_id, "bot", &p);
        v["bot"] = bot;
        self.tx(|c, ev| {
            save(c, &v)?;
            remember(c, &p, &v)?;
            ev.push(("chat.listChanged", json!(list(c)?)));
            Ok(())
        })?;
        if quick {
            self.enqueue(
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
    fn delete_chats(&self, p: &V) -> Result<V> {
        let ids = strings(&p["chatIds"]);
        let mut deleted = vec![];
        for id in ids {
            let v = self.tx(|c, _| chat(c, &id))?;
            if self
                .active
                .lock()
                .unwrap()
                .iter()
                .any(|(bot, t)| bot == &id || t.chat == id)
            {
                bail!("bot_not_ready: {id} has an active turn")
            }
            if v["kind"] == "bot" {
                let agents = self.herdr.agents()?;
                if agents
                    .iter()
                    .any(|a| a["name"] == id && a["pane_id"] == v["bot"]["paneId"])
                {
                    let pane = need(&v["bot"], "paneId")?;
                    if v["bot"]["adopted"] == true {
                        self.herdr.run(&["agent", "rename", pane, "--clear"])?;
                    } else {
                        self.herdr.run(&["pane", "close", pane])?;
                    }
                }
            }
            self.tx(|c, ev| {
                for mut room in list(c)? {
                    let mut members = strings(&room["memberIds"]);
                    if members.contains(&id) {
                        members.retain(|x| x != &id);
                        room["memberIds"] = json!(members);
                        crate::reviews::revoke_removed_members(c, ev, &room)?;
                        save(c, &room)?;
                    }
                }
                c.execute("DELETE FROM chats WHERE id=?", [&id])?;
                ev.push(("chat.listChanged", json!(list(c)?)));
                Ok(())
            })?;
            deleted.push(id);
        }
        Ok(json!({"deletedIds":deleted}))
    }
}
fn bot_cwd_unchanged(v: &V, p: &V) -> bool {
    p["cwd"].is_null() || p["cwd"] == v["bot"]["cwd"]
}
