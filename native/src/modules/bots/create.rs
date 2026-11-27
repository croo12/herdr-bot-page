use super::{service::load_defaults, working_directory::resolve_working_directory};
use crate::{
    domain::{
        bot::{
            AvatarColor, Bot, BotDefaults, BotDraft, Provider, ProviderDraft, ProviderOptions,
            SelectionFor,
        },
        need,
    },
    infrastructure::{herdr::Herdr, AgentSessions},
    storage::{BotRegistrationOptions, BotSession, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) struct CreateBotInput {
    pub profile: BotProfileDraft,
    pub options: ProviderOptions,
    pub target: BotCreationTarget,
    pub avatar_shape: Option<String>,
    pub request_id: Option<String>,
}

pub(crate) struct BotProfileDraft {
    pub id: Option<String>,
    pub name: BotName,
    pub title: Option<String>,
    pub description: Option<String>,
    pub avatar_color: Option<AvatarColor>,
}

impl BotProfileDraft {
    fn into_draft(self, provider: Provider, working_directory: Option<PathBuf>) -> BotDraft {
        let name = match self.name {
            BotName::Explicit(name) => name,
            BotName::Provider => provider.name().to_owned(),
        };
        BotDraft {
            id: self.id,
            name,
            label: self.title.unwrap_or_default(),
            instruction: self.description.unwrap_or_default(),
            avatar_color: self.avatar_color,
            provider,
            working_directory,
        }
    }
}

pub(crate) enum BotCreationTarget {
    Launch {
        provider: String,
        cwd: Option<PathBuf>,
        allow_home_cwd: bool,
    },
    Adopt {
        pane_id: String,
    },
}

pub(crate) enum BotName {
    Explicit(String),
    Provider,
}

pub(super) enum Registration {
    Created(Value),
    AlreadyCreated(Value),
}

pub(crate) fn create_bot(
    store: &Store,
    sessions: &AgentSessions,
    input: CreateBotInput,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let profile = match register_bot(store, &sessions.herdr, input)? {
        Registration::Created(profile) | Registration::AlreadyCreated(profile) => profile,
    };
    Ok(json!({"chat": profile}))
}

pub(super) fn register_bot(
    store: &Store,
    herdr: &Herdr,
    input: CreateBotInput,
) -> Result<Registration> {
    if let Some(request_id) = input.request_id.as_deref() {
        if let Some(previous_result) = store.tx(|repos, _| repos.chats.find_request(request_id))? {
            return Ok(Registration::AlreadyCreated(previous_result));
        }
    }

    let defaults = load_defaults(store)?;
    let setup = BotSetup {
        profile: input.profile,
        options: &input.options,
        defaults: &defaults,
    };

    let (bot, session) = match input.target {
        BotCreationTarget::Adopt { pane_id } => setup.adopt_bot(store, herdr, pane_id)?,
        BotCreationTarget::Launch {
            provider,
            cwd,
            allow_home_cwd,
        } => setup.launch_bot(store, herdr, provider, cwd, allow_home_cwd)?,
    };

    let options = BotRegistrationOptions {
        avatar_shape: input.avatar_shape.as_deref(),
        model: input.options.model.as_deref(),
        reasoning_effort: input.options.reasoning_effort.as_ref(),
        permission_mode: input
            .options
            .permission_mode
            .unwrap_or(defaults.permission_mode),
    };

    store.tx(|repos, events| {
        let saved_profile =
            repos
                .bots
                .save(&bot, &session, &options, input.request_id.as_deref())?;
        events.push(("chat.listChanged", json!(repos.chats.visible_list()?)));
        Ok(Registration::Created(saved_profile))
    })
}

struct BotSetup<'a> {
    profile: BotProfileDraft,
    options: &'a ProviderOptions,
    defaults: &'a BotDefaults,
}

impl BotSetup<'_> {
    fn adopt_bot(self, store: &Store, herdr: &Herdr, pane_id: String) -> Result<(Bot, BotSession)> {
        if store.tx(|repos, _| Ok(repos.chats.pane_owner(&pane_id)?.is_some()))? {
            bail!("bot_exists: pane already adopted")
        }
        let agents = herdr.ensure_and_list_agents()?;
        let agent = agents
            .iter()
            .find(|agent| agent["pane_id"] == pane_id)
            .ok_or_else(|| anyhow::anyhow!("unknown_bot: pane {pane_id}"))?;
        let provider = ProviderDraft {
            provider: need(agent, "agent")?.to_owned(),
            options: self.options.clone(),
        }
        .existing(self.defaults.permission_mode);
        let cwd = agent["cwd"].as_str().map(PathBuf::from);
        let draft = self.profile.into_draft(provider, cwd);
        let bot = Bot::new(draft, Uuid::new_v4())?;
        let bot_id = bot.id.as_str();
        let other_agents = agents.iter().filter(|agent| agent["pane_id"] != pane_id);
        ensure_bot_id_available(store, bot_id, other_agents)?;
        herdr.run(&["agent", "rename", &pane_id, bot_id])?;
        let session = BotSession {
            pane_id: Some(pane_id),
            status: agent["agent_status"].as_str().map(str::to_owned),
            adopted: true,
        };
        Ok((bot, session))
    }

    fn launch_bot(
        self,
        store: &Store,
        herdr: &Herdr,
        provider: String,
        cwd: Option<PathBuf>,
        allow_home_cwd: bool,
    ) -> Result<(Bot, BotSession)> {
        let provider = ProviderDraft {
            provider,
            options: self.options.clone(),
        }
        .select(SelectionFor::NewBot, self.defaults.permission_mode)?;
        let cwd = cwd.unwrap_or_else(|| self.defaults.working_directory.clone());
        let draft = self.profile.into_draft(provider, Some(cwd));
        let mut bot = Bot::new(draft, Uuid::new_v4())?;
        let directory =
            resolve_working_directory(bot.working_directory.as_deref(), allow_home_cwd)?;
        let agents = herdr.ensure_and_list_agents()?;
        let bot_id = bot.id.as_str();
        ensure_bot_id_available(store, bot_id, agents.iter())?;
        let agent = herdr.launch(bot_id, &directory, &bot.provider)?;
        bot.working_directory = Some(directory);
        let session = BotSession {
            pane_id: agent["pane_id"].as_str().map(str::to_owned),
            status: agent["agent_status"].as_str().map(str::to_owned),
            adopted: false,
        };
        Ok((bot, session))
    }
}

fn ensure_bot_id_available<'a>(
    store: &Store,
    bot_id: &str,
    mut other_agents: impl Iterator<Item = &'a Value>,
) -> Result<()> {
    if store.tx(|repos, _| Ok(repos.chats.find(bot_id)?.is_some()))? {
        bail!("bot_exists: {bot_id}")
    }
    if other_agents.any(|agent| agent["name"] == bot_id) {
        bail!("bot_exists: agent {bot_id} already exists")
    }
    Ok(())
}
