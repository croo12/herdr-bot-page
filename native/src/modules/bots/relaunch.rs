use super::lib::{load_defaults, resolve_working_directory};
use crate::{
    domain::{
        bot::{
            Bot, BotId, PermissionMode, ProviderDraft, ProviderOptions, ReasoningEffort,
            SelectionFor,
        },
        id,
    },
    infrastructure::{herdr::Herdr, AgentSessions},
    storage::{BotSession, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) struct RelaunchBotInput {
    pub bot_id: BotId,
    pub cwd: Option<Option<PathBuf>>,
    pub model: Option<Option<String>>,
    pub reasoning_effort: Option<Option<ReasoningEffort>>,
    pub permission_mode: Option<Option<PermissionMode>>,
    pub allow_home_cwd: bool,
}

impl RelaunchBotInput {
    fn resolve_settings(self, store: &Store, bot: &mut Bot) -> Result<PathBuf> {
        let saved = bot.provider.options();
        let permission = match self.permission_mode.unwrap_or(saved.permission_mode) {
            Some(value) => value,
            None => load_defaults(store)?.permission_mode,
        };
        let options = ProviderOptions {
            model: self.model.unwrap_or(saved.model),
            reasoning_effort: self.reasoning_effort.unwrap_or(saved.reasoning_effort),
            permission_mode: Some(permission),
        };
        bot.provider = ProviderDraft {
            provider: bot.provider.name().to_owned(),
            options,
        }
        .select(SelectionFor::Relaunch, permission)?;
        let directory = self.cwd.unwrap_or_else(|| bot.working_directory.clone());
        let allow_home = self.allow_home_cwd || directory == bot.working_directory;
        resolve_working_directory(directory.as_deref(), allow_home)
    }
}

pub(crate) fn relaunch_bot(
    store: &Store,
    sessions: &AgentSessions,
    input: RelaunchBotInput,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let registered = store.tx(|repos, _| repos.bots.registration(&input.bot_id))?;
    let mut bot = registered.bot;
    let bot_id = bot.id.as_str();
    if sessions.active.lock().unwrap().contains_key(bot_id) {
        bail!("bot_not_ready: bot is taking a turn")
    }
    let directory = input.resolve_settings(store, &mut bot)?;
    let bot_id = bot.id.as_str();
    let herdr = &sessions.herdr;
    herdr.ensure()?;
    let temporary_name = format!("hb-{}", &id()[..8]);
    let agent = herdr.launch(&temporary_name, &directory, &bot.provider)?;
    bot.working_directory = Some(directory);
    let new_pane = agent.required_pane()?;
    let old_pane = match herdr.list_agents() {
        Ok(agents) => registered.session.pane_id.as_deref().filter(|pane| {
            agents
                .iter()
                .any(|agent| agent.pane_id() == Some(*pane) && agent.name() == Some(bot_id))
        }),
        Err(error) => {
            let _ = herdr.run(&["pane", "close", new_pane]);
            return Err(error);
        }
    };
    if let Err(error) = transfer_name(herdr, bot_id, new_pane, old_pane) {
        restore_pane(herdr, bot_id, new_pane, old_pane);
        return Err(error);
    }
    let session = BotSession {
        pane_id: Some(new_pane.to_owned()),
        status: agent.status().map(str::to_owned),
        adopted: Some(false),
        ..Default::default()
    };
    let profile = match store.tx(|repos, events| repos.save_relaunch(&bot, &session, events)) {
        Ok(profile) => profile,
        Err(error) => {
            restore_pane(herdr, bot_id, new_pane, old_pane);
            return Err(error);
        }
    };
    if let Some(pane) = old_pane.filter(|_| registered.session.adopted != Some(true)) {
        herdr.run(&["pane", "close", pane])?;
    }
    Ok(json!({"chat":profile}))
}

fn transfer_name(
    herdr: &Herdr,
    name: &str,
    replacement: &str,
    previous: Option<&str>,
) -> Result<()> {
    if let Some(pane) = previous {
        herdr.run(&["agent", "rename", pane, "--clear"])?;
    }
    herdr.run(&["agent", "rename", replacement, name])?;
    Ok(())
}

fn restore_pane(herdr: &Herdr, name: &str, replacement: &str, previous: Option<&str>) {
    let _ = herdr.run(&["pane", "close", replacement]);
    if let Some(pane) = previous {
        let _ = herdr.run(&["agent", "rename", pane, name]);
    }
}
