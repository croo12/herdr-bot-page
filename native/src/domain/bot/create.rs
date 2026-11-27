use super::{AvatarColor, Bot, BotId, PermissionMode, Provider};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;

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

pub(crate) struct BotDraft {
    pub id: Option<String>,
    pub name: String,
    pub label: String,
    pub instruction: String,
    pub avatar_color: Option<AvatarColor>,
    pub provider: Provider,
    pub working_directory: Option<PathBuf>,
}

impl Bot {
    pub fn new(draft: BotDraft, id_seed: Uuid) -> Result<Self> {
        if draft.name.trim().is_empty() {
            bail!("invalid_args: missing name");
        }
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
            None => generated_id(&draft.name, id_seed),
        };
        Ok(Self {
            id: BotId::from(id.as_str()),
            name: draft.name,
            label: draft.label,
            instruction: draft.instruction,
            avatar_color: draft.avatar_color,
            working_directory: draft.working_directory,
            provider: draft.provider,
        })
    }
}

fn generated_id(name: &str, id_seed: Uuid) -> String {
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
    format!("{prefix}-{}", &id_seed.simple().to_string()[..6])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_environment_and_preference_defaults() {
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
    }

    #[test]
    fn constructs_a_bot_from_resolved_values() -> Result<()> {
        let draft = BotDraft {
            id: None,
            name: "Reviewer".into(),
            label: "Backend".into(),
            instruction: "Review the diff".into(),
            avatar_color: Some(AvatarColor::Violet),
            provider: Provider::Other("pi".into()),
            working_directory: None,
        };
        let seed = Uuid::from_u128(0xabcdef00_0000_0000_0000_000000000000);
        let bot = Bot::new(draft, seed)?;
        assert_eq!(bot.id.as_str(), "reviewer-abcdef");
        assert_eq!(bot.name, "Reviewer");
        assert_eq!(bot.label, "Backend");
        assert_eq!(bot.instruction, "Review the diff");
        assert_eq!(bot.avatar_color, Some(AvatarColor::Violet));
        assert_eq!(bot.provider, Provider::Other("pi".into()));
        assert_eq!(bot.working_directory, None);
        Ok(())
    }
}
