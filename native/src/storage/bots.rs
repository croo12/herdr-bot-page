use crate::domain::bot::{AvatarColor, Bot, BotId, LaunchSettings};
use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

pub(crate) struct Bots<'db> {
    db: &'db Connection,
}

impl<'db> Bots<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }

    pub(crate) fn last_working_directory(&self) -> Result<Option<std::path::PathBuf>> {
        let directory: Option<String> = self
            .db
            .query_row(
                "SELECT json_extract(data, '$.bot.cwd') FROM bots
             WHERE json_type(data, '$.bot.cwd') = 'text' ORDER BY rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(directory.map(Into::into))
    }

    pub(crate) fn get(&self, id: &BotId) -> Result<Bot> {
        let key = id.as_str();
        let data: String = self
            .db
            .query_row("SELECT data FROM bots WHERE id=?", [key], |row| row.get(0))
            .optional()?
            .ok_or_else(|| anyhow!("unknown_bot: {key}"))?;
        let stored: StoredBot =
            serde_json::from_str(&data).with_context(|| format!("invalid bot record: {key}"))?;
        Ok(stored.into())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredBot {
    id: BotId,
    name: String,
    title: Option<String>,
    description: Option<String>,
    avatar_color: Option<AvatarColor>,
    bot: LaunchSettings,
}

impl From<StoredBot> for Bot {
    fn from(stored: StoredBot) -> Self {
        let provider = (&stored.bot).into();
        Self {
            id: stored.id,
            name: stored.name,
            label: stored.title.unwrap_or_default(),
            instruction: stored.description.unwrap_or_default(),
            avatar_color: stored.avatar_color,
            working_directory: stored.bot.cwd,
            provider,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        bot::{CodexModelId, CodexPermissionMode, Provider, ReasoningEffort},
        new_chat,
    };
    use crate::storage::Store;
    use serde_json::{json, Value};
    use std::path::PathBuf;

    #[test]
    fn reads_the_existing_profile_without_changing_runtime_state() -> Result<()> {
        let home = tempfile::tempdir()?;
        let store = Store::open(home.path(), |_| {})?;
        let mut row = new_chat(
            "reviewer",
            "bot",
            &json!({
                "name":"Reviewer", "title":"Backend", "description":"Review every diff.", "avatarColor":"violet"
            }),
        );
        row["bot"] = json!({
            "provider":"codex", "cwd":".", "model":"retired-model", "reasoningEffort":"high",
            "permissionMode":"auto", "pendingPrompt":{"signature":"waiting"}
        });
        store.tx(|repos, _| {
            repos.chats.save(&row)?;
            let id = BotId::from("reviewer");
            assert_eq!(
                repos.bots.get(&id)?,
                Bot {
                    id,
                    name: "Reviewer".into(),
                    label: "Backend".into(),
                    instruction: "Review every diff.".into(),
                    avatar_color: Some(AvatarColor::Violet),
                    working_directory: Some(PathBuf::from(".")),
                    provider: Provider::Codex {
                        model: Some(CodexModelId("retired-model".into())),
                        reasoning_effort: Some(ReasoningEffort::High),
                        permission_mode: CodexPermissionMode::WorkspaceWriteOnRequest,
                    }
                }
            );
            assert_eq!(repos.chats.get("reviewer")?, row);
            Ok(())
        })
    }

    #[test]
    fn accepts_unset_or_empty_instructions_and_reports_a_malformed_value() -> Result<()> {
        let home = tempfile::tempdir()?;
        let store = Store::open(home.path(), |_| {})?;
        let id = BotId::from("helper");
        let mut row = json!({"id":"helper","kind":"bot","name":"Helper","bot":{"provider":"pi"}});
        store.tx(|repos, _| {
            repos.chats.save(&row)?;
            assert_eq!(repos.bots.get(&id)?.instruction, "");
            for empty in [Value::Null, json!("")] {
                row["description"] = empty;
                repos.chats.save(&row)?;
                assert_eq!(repos.bots.get(&id)?.instruction, "");
            }
            row["description"] = json!(42);
            repos.chats.save(&row)?;
            let error = repos.bots.get(&id).unwrap_err();
            assert!(error.to_string().contains("helper"));
            assert!(error.root_cause().to_string().contains("expected a string"));
            Ok(())
        })
    }
}
