use crate::domain::{
    bot::{AvatarColor, Bot, BotId, ProviderDraft},
    Events,
};
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) struct RegisteredBot {
    pub bot: Bot,
    pub session: BotSession,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BotSession {
    pub pane_id: Option<String>,
    pub status: Option<String>,
    pub adopted: Option<bool>,
    #[serde(default)]
    pub pending_prompt: Value,
}

impl BotSession {
    pub fn required_pane(&self) -> Result<&str> {
        self.pane_id
            .as_deref()
            .filter(|pane| !pane.trim().is_empty())
            .context("invalid_args: missing paneId")
    }
}

pub(crate) struct Bots<'db> {
    db: &'db Connection,
}

impl<'db> Bots<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }

    pub(crate) fn id_for_request(&self, request_id: &str) -> Result<Option<BotId>> {
        let id: Option<String> = self
            .db
            .query_row(
                "SELECT bot FROM bot_requests WHERE key=?",
                [request_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id.as_deref().map(BotId::from))
    }

    fn remember_request(&self, key: &str, bot_id: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO bot_requests(key, bot) VALUES(?, ?)",
            params![key, bot_id],
        )?;
        Ok(())
    }

    pub(super) fn save_record(&self, bot_record: &Value) -> Result<()> {
        anyhow::ensure!(bot_record["kind"] == "bot", "expected a bot profile");
        let bot_id = crate::domain::need(bot_record, "id")?;
        self.db.execute(
            "INSERT INTO bots(id, data) VALUES(?, ?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![bot_id, bot_record],
        )?;
        Ok(())
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
        Ok(self.registration(id)?.bot)
    }

    pub(crate) fn registration(&self, id: &BotId) -> Result<RegisteredBot> {
        let key = id.as_str();
        self.db
            .query_row("SELECT data FROM bots WHERE id=?", [key], |row| row.get(0))
            .optional()
            .with_context(|| format!("invalid bot record: {key}"))?
            .ok_or_else(|| anyhow!("unknown_bot: {key}"))
    }

    pub(crate) fn session(&self, id: &BotId) -> Result<BotSession> {
        let data: String = self.db.query_row(
            "SELECT COALESCE(json_extract(data,'$.bot'), '{}') FROM bots WHERE id=?",
            [id.as_str()],
            |row| row.get(0),
        )?;
        Ok(serde_json::from_str(&data)?)
    }

    pub(crate) fn registrations(&self) -> Result<Vec<RegisteredBot>> {
        // Profiles imported without launch information have no agent to poll.
        let mut query = self
            .db
            .prepare("SELECT data FROM bots WHERE json_type(data,'$.bot.provider')='text'")?;
        let rows = query.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn pane_owner(&self, pane: &str) -> Result<Option<BotId>> {
        let owner: Option<String> = self.db.query_row(
            "SELECT id FROM bots WHERE json_extract(data,'$.bot.paneId')=? ORDER BY rowid LIMIT 1",
            [pane], |row| row.get(0),
        ).optional()?;
        Ok(owner.as_deref().map(BotId::from))
    }

    pub(crate) fn require_pane_owner(&self, pane: &str) -> Result<BotId> {
        self.pane_owner(pane)?
            .ok_or_else(|| anyhow!("unknown_bot: pane {pane}"))
    }

    pub(crate) fn bound_panes(&self) -> Result<HashSet<String>> {
        let mut statement = self.db.prepare(
            "SELECT json_extract(data,'$.bot.paneId') FROM bots WHERE json_type(data,'$.bot.paneId')='text'",
        )?;
        let panes = statement.query_map([], |row| row.get(0))?;
        Ok(panes.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn contains(&self, id: &BotId) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM bots WHERE id=?)",
            [id.as_str()],
            |row| row.get(0),
        )?)
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
    avatar_shape: Option<String>,
    bot: StoredBotFacts,
}

#[derive(Deserialize)]
struct StoredBotFacts {
    cwd: Option<std::path::PathBuf>,
    #[serde(flatten)]
    session: BotSession,
    #[serde(flatten)]
    provider: ProviderDraft,
}

impl rusqlite::types::FromSql for RegisteredBot {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        serde_json::from_str::<StoredBot>(value.as_str()?)
            .map(Into::into)
            .map_err(|error| rusqlite::types::FromSqlError::Other(Box::new(error)))
    }
}

impl From<StoredBot> for RegisteredBot {
    fn from(stored: StoredBot) -> Self {
        let bot = Bot {
            id: stored.id,
            name: stored.name,
            label: stored.title.unwrap_or_default(),
            instruction: stored.description.unwrap_or_default(),
            avatar_color: stored.avatar_color,
            avatar_shape: stored.avatar_shape,
            working_directory: stored.bot.cwd,
            provider: stored.bot.provider.existing(None),
        };
        Self {
            bot,
            session: stored.bot.session,
        }
    }
}

fn write_profile(profile: &mut Value, bot: &Bot) {
    profile["name"] = json!(bot.name);
    profile["title"] = json!(bot.label);
    profile["description"] = json!(bot.instruction);
    profile["avatarColor"] = json!(bot.avatar_color);
    profile["avatarShape"] = json!(bot.avatar_shape);
}

fn write_launch_settings(facts: &mut Value, bot: &Bot, session: &BotSession) {
    let options = bot.provider.options();
    facts["provider"] = json!(bot.provider.name());
    facts["model"] = json!(options.model);
    facts["reasoningEffort"] = json!(options.reasoning_effort);
    facts["permissionMode"] = json!(options.permission_mode);
    facts["cwd"] = json!(bot.working_directory);
    facts["paneId"] = json!(session.pane_id);
    facts["status"] = json!(session.status);
    facts["adopted"] = json!(session.adopted);
    facts["pendingPrompt"] = session.pending_prompt.clone();
}

impl super::Repositories<'_> {
    pub(crate) fn save_new_bot(
        &self,
        bot: &Bot,
        session: &BotSession,
        request_id: Option<&str>,
        events: &mut Events,
    ) -> Result<()> {
        let bot_id = bot.id.as_str();
        let mut profile = crate::domain::new_chat(bot_id, "bot", &Value::Null);
        write_profile(&mut profile, bot);
        profile["bot"] = json!({"onboarding": null});
        write_launch_settings(&mut profile["bot"], bot, session);
        self.bots.save_record(&profile)?;
        if let Some(key) = request_id {
            self.bots.remember_request(key, bot_id)?;
        }
        events.push(("chat.listChanged", json!(self.chats.visible_list()?)));
        Ok(())
    }

    pub(crate) fn update_bot_profile(&self, bot: &Bot, events: &mut Events) -> Result<Value> {
        let mut profile = self.chats.get(bot.id.as_str())?;
        write_profile(&mut profile, bot);
        self.chats.save_changed(&profile, events)?;
        Ok(profile)
    }

    pub(crate) fn update_bot_status(
        &self,
        id: &BotId,
        status: &str,
        prompt: Value,
        events: &mut Events,
    ) -> Result<()> {
        let mut row = self.chats.get(id.as_str())?;
        row["bot"]["status"] = status.into();
        row["bot"]["pendingPrompt"] = prompt;
        row["isRunning"] = (status == "working").into();
        row["currentActivity"] = if status == "working" {
            json!({"verb":"working"})
        } else {
            Value::Null
        };
        row["awaitingUserResponse"] = match status {
            "blocked" => json!({"reason":"approval"}),
            "offline" => json!({"reason":"offline"}),
            _ => Value::Null,
        };
        self.chats.save_changed(&row, events)
    }

    pub(crate) fn rebind_bot_pane(
        &self,
        id: &BotId,
        pane: &str,
        events: &mut Events,
    ) -> Result<()> {
        let owners = {
            let mut statement = self.bots.db.prepare(
                "SELECT id FROM bots WHERE id=?1 OR json_extract(data,'$.bot.paneId')=?2",
            )?;
            let rows = statement.query_map(params![id.as_str(), pane], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
        };
        for owner in owners {
            if let Some((room, mut prompt)) = self.prompts.pending(&owner)? {
                prompt["status"] = "resolved".into();
                self.update_entry(&room, &prompt, events)?;
            }
            let mut profile = self.chats.get(&owner)?;
            profile["bot"]["paneId"] = json!((owner == id.as_str()).then_some(pane));
            profile["bot"]["pendingPrompt"] = Value::Null;
            self.chats.save_changed(&profile, events)?;
        }
        Ok(())
    }

    pub(crate) fn save_relaunch(
        &self,
        bot: &Bot,
        session: &BotSession,
        events: &mut Events,
    ) -> Result<()> {
        let mut profile = self.chats.get(bot.id.as_str())?;
        write_launch_settings(&mut profile["bot"], bot, session);
        self.chats.save_changed(&profile, events)
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
    fn rebinding_a_reused_pane_clears_only_the_affected_sessions() -> Result<()> {
        let home = tempfile::tempdir()?;
        let store = Store::open(home.path(), |_| {})?;
        store.tx(|repos, events| {
            for (owner, pane) in [("reviewer","p1"), ("previous","p2"), ("other","p3")] {
                let mut profile = new_chat(owner, "bot", &json!({"name":owner}));
                profile["bot"] = json!({"provider":"pi", "paneId":pane, "pendingPrompt":{"signature":owner}, "extra":"preserve"});
                repos.bots.save_record(&profile)?;
            }
            repos.rebind_bot_pane(&BotId::from("reviewer"), "p2", events)?;
            let owner = repos.chats.by_pane("p2")?;
            assert_eq!(owner["id"], "reviewer");
            assert!(owner["bot"]["pendingPrompt"].is_null());
            assert_eq!(owner["bot"]["extra"], "preserve");
            let previous = repos.chats.get("previous")?;
            assert!(previous["bot"]["paneId"].is_null());
            assert!(previous["bot"]["pendingPrompt"].is_null());
            assert_eq!(repos.chats.by_pane("p3")?["bot"]["pendingPrompt"]["signature"], "other");
            Ok(())
        })
    }

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
                    avatar_shape: None,
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
