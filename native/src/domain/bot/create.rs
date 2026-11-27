use super::model::{AvatarColor, Bot, BotId, LaunchSettings, PermissionMode, ReasoningEffort};
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
    provider: String,
    cwd: Option<PathBuf>,
    permission_mode: Option<PermissionMode>,
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
}

impl BotDraft {
    pub fn launch_settings(&self, defaults: &BotDefaults) -> LaunchSettings {
        LaunchSettings {
            provider: self.provider.clone(),
            cwd: Some(
                self.cwd
                    .clone()
                    .unwrap_or_else(|| defaults.working_directory.clone()),
            ),
            permission_mode: Some(self.permission_mode.unwrap_or(defaults.permission_mode)),
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
        }
    }
}

impl Bot {
    pub fn new(draft: BotDraft, settings: &LaunchSettings, quick: bool) -> Result<Self> {
        let name = match draft.name {
            Some(name) if !name.trim().is_empty() => name,
            _ if quick => settings.provider.clone(),
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
            working_directory: settings.cwd.clone(),
            provider: settings.into(),
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
            "provider":"codex", "cwd":"explicit", "permissionMode":"ask", "model":"selected-model"
        }))?;
        let settings = draft.launch_settings(&defaults);
        let bot = Bot::new(draft, &settings, true)?;
        assert_eq!(bot.name, "codex");
        assert!(bot.id.as_str().starts_with("codex-"));
        assert_eq!(bot.working_directory, Some(PathBuf::from("explicit")));
        assert_eq!(
            bot.provider,
            Provider::Codex {
                model: Some(CodexModelId("selected-model".into())),
                reasoning_effort: None,
                permission_mode: CodexPermissionMode::Default,
            }
        );
        Ok(())
    }
}
