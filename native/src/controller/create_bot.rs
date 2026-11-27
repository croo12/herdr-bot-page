use crate::{
    domain::bot::{AvatarColor, PermissionMode, ProviderOptions, ReasoningEffort},
    modules::bots::{BotCreationTarget, BotName, BotProfileDraft, CreateBotInput},
};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreateBotRequest {
    id: Option<String>,
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    avatar_color: Option<AvatarColor>,
    avatar_shape: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    permission_mode: Option<PermissionMode>,
    reasoning_effort: Option<ReasoningEffort>,
    adopt_pane_id: Option<String>,
    cwd: Option<PathBuf>,
    allow_home_cwd: Option<bool>,
    request_id: Option<String>,
    pub(super) locale: Option<String>,
}

impl CreateBotRequest {
    pub(super) fn into_input(self) -> Result<CreateBotInput> {
        let target = match self.adopt_pane_id {
            Some(pane_id) => {
                ensure!(
                    !pane_id.trim().is_empty(),
                    "invalid_args: missing adoptPaneId"
                );
                BotCreationTarget::Adopt { pane_id }
            }
            None => BotCreationTarget::Launch {
                provider: self
                    .provider
                    .filter(|name| !name.trim().is_empty())
                    .context("invalid_args: missing provider")?,
                cwd: self.cwd,
                allow_home_cwd: self.allow_home_cwd.unwrap_or(false),
            },
        };
        Ok(CreateBotInput {
            profile: BotProfileDraft {
                id: self.id,
                name: BotName::Explicit(self.name.unwrap_or_default()),
                title: self.title,
                description: self.description,
                avatar_color: self.avatar_color,
            },
            options: ProviderOptions {
                model: self.model,
                reasoning_effort: self.reasoning_effort,
                permission_mode: self.permission_mode,
            },
            target,
            avatar_shape: self.avatar_shape,
            request_id: self.request_id,
        })
    }
}
