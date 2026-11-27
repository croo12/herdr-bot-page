use super::{AvatarColor, Bot, BotId, PermissionMode, ProviderDraft, SelectionFor};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotPreferences {
    default_working_folder: Option<PathBuf>,
    default_provider: Option<String>,
    default_permission_mode: Option<PermissionMode>,
}

pub(crate) struct BotDefaults {
    pub working_directory: PathBuf,
    pub provider: String,
    pub permission_mode: PermissionMode,
}

impl BotDefaults {
    pub fn resolve(
        preferences: BotPreferences,
        environment_directory: Option<PathBuf>,
        environment_provider: Option<String>,
        last_directory: Option<PathBuf>,
    ) -> Self {
        Self {
            working_directory: environment_directory
                .or(preferences.default_working_folder)
                .or(last_directory)
                .unwrap_or_default(),
            provider: preferences
                .default_provider
                .or(environment_provider)
                .unwrap_or_else(|| "claude".into()),
            permission_mode: preferences
                .default_permission_mode
                .unwrap_or(PermissionMode::Ask),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotDraft {
    id: Option<String>,
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    avatar_color: Option<AvatarColor>,
    #[serde(flatten)]
    provider: ProviderDraft,
    cwd: Option<PathBuf>,
}

pub(crate) enum BotOrigin {
    New,
    Adopted,
}

impl Bot {
    pub fn new(
        draft: BotDraft,
        defaults: &BotDefaults,
        quick: bool,
        origin: BotOrigin,
    ) -> Result<Self> {
        let (working_directory, provider) = match origin {
            BotOrigin::New => (
                Some(
                    draft
                        .cwd
                        .unwrap_or_else(|| defaults.working_directory.clone()),
                ),
                draft
                    .provider
                    .select(SelectionFor::NewBot, defaults.permission_mode)?,
            ),
            BotOrigin::Adopted => (draft.cwd, draft.provider.existing(defaults.permission_mode)),
        };
        let name = match draft.name {
            Some(name) if !name.trim().is_empty() => name,
            _ if quick => provider.name().to_owned(),
            _ => bail!("invalid_args: missing name"),
        };
        let id = match draft.id {
            Some(id) => {
                if !id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                    || id.len() > 32
                    || id.starts_with("room-")
                    || !id.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'-' | b'_')
                    })
                {
                    bail!("invalid_bot_id: {id}");
                }
                id
            }
            None => generated_id(&name),
        };
        Ok(Self {
            id: BotId::from(id.as_str()),
            name,
            label: draft.title.unwrap_or_default(),
            instruction: draft.description.unwrap_or_default(),
            avatar_color: draft.avatar_color,
            working_directory,
            provider,
        })
    }
}

fn generated_id(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .take(20)
        .collect();
    let slug = slug.trim_matches('-');
    let prefix = if slug.starts_with(|character: char| character.is_ascii_lowercase())
        && !slug.starts_with("room-")
    {
        slug
    } else {
        "bot"
    };
    format!("{prefix}-{}", &crate::domain::id()[..6])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bot::{CodexModelId, CodexPermissionMode, Provider};

    #[test]
    fn explicit_settings_override_the_defaults_shown_before_creation() -> Result<()> {
        let defaults = BotDefaults::resolve(
            BotPreferences {
                default_working_folder: Some("configured".into()),
                default_provider: Some("claude".into()),
                default_permission_mode: Some(PermissionMode::Auto),
            },
            Some("environment".into()),
            Some("grok".into()),
            Some("previous".into()),
        );
        assert_eq!(defaults.working_directory, PathBuf::from("environment"));
        assert_eq!(defaults.provider, "claude");
        let draft: BotDraft = serde_json::from_value(serde_json::json!({
            "provider":"codex", "cwd":"explicit", "permissionMode":"ask", "model":"selected-model", "reasoningEffort":"high"
        }))?;
        let bot = Bot::new(draft, &defaults, true, BotOrigin::New)?;
        assert_eq!(bot.name, "codex");
        assert!(bot.id.as_str().starts_with("codex-"));
        assert_eq!(bot.working_directory, Some(PathBuf::from("explicit")));
        assert_eq!(
            bot.provider,
            Provider::Codex {
                model: Some(CodexModelId("selected-model".into())),
                reasoning_effort: Some(crate::domain::bot::ReasoningEffort::High),
                permission_mode: CodexPermissionMode::Default,
            }
        );
        Ok(())
    }
}
