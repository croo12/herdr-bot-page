use super::{Repositories, Store};
use crate::domain::{id, now, record_latest_entry, Events};
use anyhow::Result;
use serde_json::{json, Value};

impl Repositories<'_> {
    pub(crate) fn append_entry(
        &self,
        chat_id: &str,
        mut entry: Value,
        events: &mut Events,
    ) -> Result<Value> {
        let mut chat = self.chats.get(chat_id)?;
        let seq = self.transcript.latest_seq(chat_id)? + 1;
        let timestamp = now();
        entry["seq"] = seq.into();
        entry["id"] = id().into();
        entry["timestampMs"] = timestamp.into();
        self.transcript.insert(chat_id, seq, &entry)?;
        record_latest_entry(&mut chat, &entry);
        events.push((
            "transcript.entryAppended",
            json!({"chatId":chat_id,"entry":entry}),
        ));
        self.chats.save_changed(&chat, events)?;
        Ok(entry)
    }
    pub(crate) fn update_entry(
        &self,
        chat_id: &str,
        entry: &Value,
        events: &mut Events,
    ) -> Result<()> {
        self.transcript.update(chat_id, entry)?;
        events.push((
            "transcript.entryUpdated",
            json!({"chatId":chat_id,"entry":entry}),
        ));
        Ok(())
    }
}

impl Store {
    pub(crate) fn notice(&self, target: &str, text: &str) {
        let _ = self.tx(|repos, events| {
            repos.append_entry(target, json!({"kind":"notice","content":text}), events)
        });
    }
}
