use crate::domain::{
    content, need, now, strings, ChatJob, DelegationUpdate, Delivery, MessageSource, TurnContext,
};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) struct Inbox<'db>(&'db Connection);

impl<'db> Inbox<'db> {
    pub fn new(db: &'db Connection) -> Self {
        Self(db)
    }

    pub fn deliver(
        &self,
        chat: &Value,
        entry: &Value,
        source: MessageSource<'_>,
        depth: u8,
    ) -> Result<Vec<String>> {
        let mut recipients = Vec::new();
        let members = strings(&chat["memberIds"]);
        let text = content(entry);
        let rich: Option<Value> = entry["richText"]
            .as_str()
            .and_then(|rich_text| serde_json::from_str(rich_text).ok());
        let chips: Vec<_> = rich
            .iter()
            .flat_map(|doc| doc["content"].as_array().into_iter().flatten())
            .flat_map(|block| block["content"].as_array().into_iter().flatten())
            .filter(|node| node["type"] == "mention")
            .map(|node| &node["attrs"])
            .collect();
        let (mentions, all, addressed) = if chips.is_empty() {
            let mentions: Vec<_> = text
                .split(|character: char| {
                    !(character.is_ascii_alphanumeric() || matches!(character, '@' | '-' | '_'))
                })
                .filter_map(|word| word.strip_prefix('@'))
                .collect();
            let all = mentions
                .iter()
                .any(|id| id.eq_ignore_ascii_case("everyone") || id.eq_ignore_ascii_case("all"));
            let addressed = members.iter().any(|id| mentions.contains(&id.as_str()));
            (mentions, all, addressed)
        } else {
            let mentions = chips
                .iter()
                .filter_map(|attrs| attrs["id"].as_str())
                .collect();
            (
                mentions,
                chips.iter().any(|attrs| attrs["everyone"] == true),
                true,
            )
        };
        let room = chat["kind"] == "room";
        let broadcast = all || (!addressed && !matches!(source, MessageSource::Bot(_)));
        let reply_author = match (room, source, entry["replyTo"].as_str()) {
            (true, MessageSource::Bot(_), Some(parent_id)) => {
                let parent = super::Transcript::new(self.0).by_id(need(chat, "id")?, parent_id)?;
                if parent["kind"] == "send-message" {
                    parent["author"]["id"].as_str().map(str::to_owned)
                } else {
                    None
                }
            }
            _ => None,
        };
        for bot in members {
            if source.bot_id() == Some(bot.as_str()) {
                continue;
            }
            if room
                && !broadcast
                && reply_author.as_deref() != Some(bot.as_str())
                && !mentions.contains(&bot.as_str())
            {
                continue;
            }
            self.0.execute(
                "INSERT INTO inbox(bot,chat,seq,depth) VALUES(?,?,?,?)",
                params![bot, need(chat, "id")?, entry["seq"], depth],
            )?;
            recipients.push(bot);
        }
        Ok(recipients)
    }

    pub fn list(&self, bot: &str) -> Result<Vec<Value>> {
        let mut query = self.0.prepare("SELECT i.chat,e.data,i.state FROM inbox i JOIN entries e ON e.chat=i.chat AND e.seq=i.seq WHERE i.bot=? ORDER BY i.rowid")?;
        let rows = query.query_map([bot], |row| {
            let chat: String = row.get(0)?;
            let entry: Value = row.get(1)?;
            let state:String=row.get(2)?;
            Ok(json!({"chatId":chat,"messageId":entry["id"],"author":entry["author"],"text":content(&entry),"state":state}))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn ready_bots(&self) -> Result<Vec<String>> {
        let mut query = self.0.prepare(
            "SELECT DISTINCT bot FROM inbox WHERE state='pending' AND available<=? AND depth<=2",
        )?;
        let rows = query.query_map([now()], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn scheduled_chats(&self) -> Result<Vec<String>> {
        let mut query = self.0.prepare(
            "SELECT DISTINCT chat FROM inbox WHERE state IN ('queued','working')
             OR (state='pending' AND available<=? AND depth<=2)",
        )?;
        let rows = query.query_map([now()], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // Claim the oldest eligible message across this bot's subscriptions in the caller's transaction.
    pub fn reserve_next(&self, bot: &str) -> Result<Option<(String, ChatJob)>> {
        let row = self.0.query_row(
            "SELECT i.chat,i.seq,i.depth,e.data FROM inbox i JOIN entries e ON e.chat=i.chat AND e.seq=i.seq
             WHERE i.rowid=(SELECT rowid FROM inbox WHERE bot=? AND state='pending' AND depth<=2 ORDER BY rowid LIMIT 1)
               AND i.available<=?",
            params![bot, now()],
            |row| {
                let chat: String = row.get(0)?;
                let seq: i64 = row.get(1)?;
                let depth: u8 = row.get(2)?;
                let entry: Value = row.get(3)?;
                Ok((chat, seq, depth, entry))
            },
        ).optional()?;
        let Some((chat, seq, depth, entry)) = row else {
            return Ok(None);
        };
        self.0.execute(
            "UPDATE inbox SET state='queued' WHERE bot=? AND chat=? AND seq=?",
            params![bot, chat, seq],
        )?;
        let job = ChatJob {
            delivery: Some(Delivery {
                bot: bot.into(),
                chat: chat.clone(),
                seq,
            }),
            context: TurnContext::from_entry(&chat, &entry),
            seed: content(&entry).into(),
            depth,
        };
        Ok(Some((chat, job)))
    }

    pub fn start(&self, delivery: &Delivery) -> Result<bool> {
        Ok(self.0.execute(
            "UPDATE inbox SET state='working' WHERE bot=? AND chat=? AND seq=? AND state='queued'",
            params![delivery.bot, delivery.chat, delivery.seq],
        )? == 1)
    }
    pub fn resolve(&self, bot: &str, message: &str, retry: bool) -> Result<Delivery> {
        let condition=" WHERE bot=? AND (chat,seq) IN (SELECT chat,seq FROM entries WHERE json_extract(data,'$.id')=?) AND state IN ('pending','unconfirmed')";
        let operation = if retry {
            "UPDATE inbox SET state='pending',available=0,last_error=NULL,depth=min(depth,2)"
        } else {
            "DELETE FROM inbox"
        };
        self.0
            .query_row(
                &format!("{operation}{condition} RETURNING chat,seq"),
                [bot, message],
                |row| {
                    Ok(Delivery {
                        bot: bot.into(),
                        chat: row.get(0)?,
                        seq: row.get(1)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("invalid_args: no matching inactive delivery"))
    }
    pub fn delegation_progress(&self, delivery: &Delivery) -> Result<DelegationUpdate> {
        Ok(self.0.query_row(
            "SELECT count(*),count(*) FILTER (WHERE state='working')>0,
             max(CASE WHEN state='unconfirmed' THEN coalesce(last_error,'Delivery was not confirmed') ELSE last_error END)
             FROM inbox WHERE chat=? AND seq=?",
            params![delivery.chat, delivery.seq],
            |row| {
                let remaining: usize = row.get(0)?;
                let working: bool = row.get(1)?;
                let last_error: Option<String> = row.get(2)?;
                Ok(match (working, last_error, remaining) {
                    (true, last_error, _) => DelegationUpdate::Running { last_error },
                    (false, Some(error), _) => DelegationUpdate::Failed(error),
                    (false, None, 0) => DelegationUpdate::Completed,
                    (false, None, _) => DelegationUpdate::Queued,
                })
            },
        )?)
    }

    pub fn finish(&self, delivery: &Delivery) -> Result<()> {
        self.0.execute(
            "DELETE FROM inbox WHERE bot=? AND chat=? AND seq=?",
            params![delivery.bot, delivery.chat, delivery.seq],
        )?;
        Ok(())
    }
    pub fn release(&self, delivery: &Delivery, error: Option<&str>) -> Result<bool> {
        let report = if let Some(message) = error {
            self.0.execute("UPDATE inbox SET last_error=? WHERE bot=? AND chat=? AND seq=? AND last_error IS NOT ?",
                params![message,delivery.bot,delivery.chat,delivery.seq,message])? > 0
        } else {
            false
        };
        self.0.execute(
            "UPDATE inbox SET available=?,state=CASE state WHEN 'working' THEN 'unconfirmed' ELSE 'pending' END WHERE bot=? AND chat=? AND seq=? AND state IN ('queued','working')",
            params![now() + if error.is_some(){30_000}else{3_000}, delivery.bot, delivery.chat, delivery.seq],
        )?;
        Ok(report)
    }
    pub fn keep_members(&self, chat: &Value) -> Result<()> {
        self.0.execute(
            "DELETE FROM inbox WHERE chat=? AND bot NOT IN (SELECT value FROM json_each(?))",
            params![need(chat, "id")?, chat["memberIds"]],
        )?;
        Ok(())
    }
}
