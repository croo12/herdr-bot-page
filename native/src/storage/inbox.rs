use crate::domain::{content, need, now, strings, ChatJob, Delivery};
use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::{json, Value};

pub(crate) struct Inbox<'db>(&'db Connection);

impl super::Store {
    pub(crate) fn schedule_inbox(&self, enqueue: &dyn Fn(&str, ChatJob)) -> Result<()> {
        for job in self.tx(|repos, _| repos.inbox.reserve())? {
            let chat = job.delivery.as_ref().unwrap().chat.clone();
            enqueue(&chat, job);
        }
        Ok(())
    }
}

impl<'db> Inbox<'db> {
    pub fn new(db: &'db Connection) -> Self {
        Self(db)
    }

    pub fn deliver(
        &self,
        chat: &Value,
        entry: &Value,
        sender: Option<&str>,
        depth: u8,
        delegate: bool,
    ) -> Result<()> {
        let members = strings(&chat["memberIds"]);
        let text = content(entry);
        let rich: Option<Value> = entry["richText"]
            .as_str()
            .and_then(|s| serde_json::from_str(s).ok());
        let chips: Vec<_> = rich
            .iter()
            .flat_map(|doc| doc["content"].as_array().into_iter().flatten())
            .flat_map(|block| block["content"].as_array().into_iter().flatten())
            .filter(|node| node["type"] == "mention")
            .map(|node| &node["attrs"])
            .collect();
        let mut mentions: Vec<_> = text
            .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '@' | '-' | '_')))
            .filter_map(|word| word.strip_prefix('@'))
            .collect();
        let all = if chips.is_empty() {
            mentions
                .iter()
                .any(|id| id.eq_ignore_ascii_case("everyone") || id.eq_ignore_ascii_case("all"))
        } else {
            chips.iter().any(|attrs| attrs["everyone"] == true)
        };
        let addressed = if chips.is_empty() {
            members.iter().any(|id| mentions.contains(&id.as_str()))
        } else {
            mentions = chips
                .iter()
                .filter_map(|attrs| attrs["id"].as_str())
                .collect();
            true
        };
        for bot in members {
            if sender == Some(bot.as_str()) {
                continue;
            }
            if chat["kind"] == "room"
                && !all
                && ((addressed && !mentions.contains(&bot.as_str()))
                    || (!addressed && sender.is_some() && !delegate))
            {
                continue;
            }
            self.0.execute(
                "INSERT INTO inbox(bot,chat,seq,depth) VALUES(?,?,?,?)",
                params![bot, need(chat, "id")?, entry["seq"], depth],
            )?;
        }
        Ok(())
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

    // The host holds the database transaction while reserving jobs and is the only scheduler.
    // A fresh host clears abandoned reservations before it starts the scheduler.
    pub fn reserve(&self) -> Result<Vec<ChatJob>> {
        let mut query = self.0.prepare("SELECT i.bot,i.chat,i.seq,i.depth,e.data FROM inbox i JOIN entries e ON e.chat=i.chat AND e.seq=i.seq WHERE i.state='pending' AND i.available<=? AND i.depth<=2 ORDER BY i.rowid")?;
        let rows = query.query_map([now()], |r| {
            let entry: Value = r.get(4)?;
            Ok(ChatJob {
                delivery: Some(Delivery {
                    bot: r.get(0)?,
                    chat: r.get(1)?,
                    seq: r.get(2)?,
                }),
                seed: content(&entry).into(),
                depth: r.get(3)?,
            })
        })?;
        let jobs = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for job in &jobs {
            let d = job.delivery.as_ref().unwrap();
            self.0.execute(
                "UPDATE inbox SET state='queued' WHERE bot=? AND chat=? AND seq=?",
                params![d.bot, d.chat, d.seq],
            )?;
        }
        Ok(jobs)
    }

    pub fn start(&self, d: &Delivery) -> Result<bool> {
        Ok(self.0.execute(
            "UPDATE inbox SET state='working' WHERE bot=? AND chat=? AND seq=? AND state='queued'",
            params![d.bot, d.chat, d.seq],
        )? == 1)
    }
    pub fn resolve(&self, bot: &str, message: &str, retry: bool) -> Result<()> {
        let condition=" WHERE bot=? AND (chat,seq) IN (SELECT chat,seq FROM entries WHERE json_extract(data,'$.id')=?) AND state IN ('pending','unconfirmed')";
        let operation = if retry {
            "UPDATE inbox SET state='pending',available=0,depth=min(depth,2)"
        } else {
            "DELETE FROM inbox"
        };
        anyhow::ensure!(
            self.0
                .execute(&format!("{operation}{condition}"), [bot, message])?
                == 1,
            "invalid_args: no matching inactive delivery"
        );
        Ok(())
    }
    pub fn finish(&self, d: &Delivery) -> Result<()> {
        self.0.execute(
            "DELETE FROM inbox WHERE bot=? AND chat=? AND seq=?",
            params![d.bot, d.chat, d.seq],
        )?;
        Ok(())
    }
    pub fn release(&self, d: &Delivery, error: Option<&str>) -> Result<bool> {
        let report = if let Some(message) = error {
            self.0.execute("UPDATE inbox SET last_error=? WHERE bot=? AND chat=? AND seq=? AND last_error IS NOT ?",
                params![message,d.bot,d.chat,d.seq,message])? > 0
        } else {
            false
        };
        self.0.execute(
            "UPDATE inbox SET available=?,state=CASE state WHEN 'working' THEN 'unconfirmed' ELSE 'pending' END WHERE bot=? AND chat=? AND seq=? AND state IN ('queued','working')",
            params![now() + if error.is_some(){30_000}else{3_000}, d.bot, d.chat, d.seq],
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
