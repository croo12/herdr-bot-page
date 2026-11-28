use super::{load_defaults, resolve_working_directory};
use crate::{
    domain::bot::{
        AvatarColor, Bot, BotDefaults, BotDraft, BotId, Provider, ProviderDraft, ProviderOptions,
        SelectionFor,
    },
    infrastructure::herdr::{Herdr, HerdrAgent},
    storage::{BotSession, Store},
};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) struct CreateBotInput {
    pub profile: BotProfileDraft,
    pub options: ProviderOptions,
    pub target: BotCreationTarget,
    pub request_id: Option<String>,
}

pub(crate) struct BotProfileDraft {
    pub id: Option<String>,
    pub name: BotName,
    pub title: Option<String>,
    pub description: Option<String>,
    pub avatar_color: Option<AvatarColor>,
    pub avatar_shape: Option<String>,
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
            avatar_shape: self.avatar_shape,
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

pub(in crate::modules::bots) enum Registration {
    Created(BotId),
    AlreadyCreated(BotId),
}

pub(in crate::modules::bots) fn register_bot(
    store: &Store,
    herdr: &Herdr,
    input: CreateBotInput,
) -> Result<Registration> {
    if let Some(request_id) = input.request_id.as_deref() {
        if let Some(bot_id) = store.tx(|repos, _| repos.bots.id_for_request(request_id))? {
            return Ok(Registration::AlreadyCreated(bot_id));
        }
    }

    let defaults = load_defaults(store)?;
    let setup = BotSetup {
        profile: input.profile,
        options: input.options,
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

    store.tx(|repos, events| {
        repos.save_new_bot(&bot, &session, input.request_id.as_deref(), events)
    })?;
    Ok(Registration::Created(bot.id))
}

struct BotSetup<'a> {
    profile: BotProfileDraft,
    options: ProviderOptions,
    defaults: &'a BotDefaults,
}

impl BotSetup<'_> {
    fn adopt_bot(self, store: &Store, herdr: &Herdr, pane_id: String) -> Result<(Bot, BotSession)> {
        if store.tx(|repos, _| Ok(repos.bots.pane_owner(&pane_id)?.is_some()))? {
            bail!("bot_exists: pane already adopted")
        }
        let agents = herdr.ensure_and_list_agents()?;
        let agent = agents
            .iter()
            .find(|agent| agent.pane_id() == Some(pane_id.as_str()))
            .ok_or_else(|| anyhow::anyhow!("unknown_bot: pane {pane_id}"))?;
        let provider = ProviderDraft {
            provider: agent
                .provider()
                .filter(|name| !name.trim().is_empty())
                .context("invalid_args: missing agent")?
                .to_owned(),
            options: self.options,
        }
        .existing(Some(self.defaults.permission_mode));
        let cwd = agent.cwd().map(PathBuf::from);
        let draft = self.profile.into_draft(provider, cwd);
        let bot = Bot::new(draft, Uuid::new_v4())?;
        let bot_id = bot.id.as_str();
        let other_agents = agents
            .iter()
            .filter(|agent| agent.pane_id() != Some(pane_id.as_str()));
        ensure_bot_id_available(store, &bot.id, other_agents)?;
        herdr.run(&["agent", "rename", &pane_id, bot_id])?;
        let session = BotSession {
            pane_id: Some(pane_id),
            status: agent.status().map(str::to_owned),
            adopted: Some(true),
            ..Default::default()
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
            options: self.options,
        }
        .select(SelectionFor::NewBot, self.defaults.permission_mode)?;
        let cwd = cwd.unwrap_or_else(|| self.defaults.working_directory.clone());
        let draft = self.profile.into_draft(provider, Some(cwd));
        let mut bot = Bot::new(draft, Uuid::new_v4())?;
        let directory =
            resolve_working_directory(bot.working_directory.as_deref(), allow_home_cwd)?;
        let agents = herdr.ensure_and_list_agents()?;
        let bot_id = bot.id.as_str();
        ensure_bot_id_available(store, &bot.id, agents.iter())?;
        let agent = herdr.launch(bot_id, &directory, &bot.provider)?;
        bot.working_directory = Some(directory);
        let session = BotSession {
            pane_id: agent.pane_id().map(str::to_owned),
            status: agent.status().map(str::to_owned),
            adopted: Some(false),
            ..Default::default()
        };
        Ok((bot, session))
    }
}

fn ensure_bot_id_available<'a>(
    store: &Store,
    id: &BotId,
    mut other_agents: impl Iterator<Item = &'a HerdrAgent>,
) -> Result<()> {
    let bot_id = id.as_str();
    if store.tx(|repos, _| repos.chats.contains_id(bot_id))? {
        bail!("bot_exists: {bot_id}")
    }
    if other_agents.any(|agent| agent.name() == Some(bot_id)) {
        bail!("bot_exists: agent {bot_id} already exists")
    }
    Ok(())
}
