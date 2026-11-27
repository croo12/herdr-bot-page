use super::{Repositories, Store};
use crate::domain::{
    id, now, record_latest_entry, DelegationRef, DelegationUpdate, Delivery, Events, TurnContext,
};
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
        if crate::domain::visible_to(&chat, None) {
            events.push((
                "transcript.entryAppended",
                json!({"chatId":chat_id,"entry":entry}),
            ));
        }
        self.chats.save_changed(&chat, events)?;
        Ok(entry)
    }
    pub(crate) fn finish_delivery(
        &self,
        delivery: &Delivery,
        link: Option<&DelegationRef>,
        events: &mut Events,
    ) -> Result<()> {
        self.inbox.finish(delivery)?;
        self.sync_delivery_state(delivery, link, events)
    }

    pub(crate) fn resolve_delivery(
        &self,
        bot: &str,
        message: &str,
        retry: bool,
        events: &mut Events,
    ) -> Result<()> {
        let delivery = self.inbox.resolve(bot, message, retry)?;
        let entry = self.transcript.by_id(&delivery.chat, message)?;
        let context = TurnContext::from_entry(&delivery.chat, &entry);
        self.sync_delivery_state(&delivery, context.delegation.as_ref(), events)
    }

    pub(crate) fn sync_delivery_state(
        &self,
        delivery: &Delivery,
        link: Option<&DelegationRef>,
        events: &mut Events,
    ) -> Result<()> {
        if let Some(link) = link {
            let update = self.inbox.delegation_progress(delivery)?;
            self.set_delegation_state(link, update, events)?;
        }
        Ok(())
    }
    pub(crate) fn set_delegation_state(
        &self,
        link: &DelegationRef,
        update: DelegationUpdate,
        events: &mut Events,
    ) -> Result<()> {
        // The source conversation can be deleted while its worker finishes in another chat.
        if !self.transcript.contains(&link.chat_id, &link.entry_id)? {
            return Ok(());
        }
        let mut entry = self.transcript.by_id(&link.chat_id, &link.entry_id)?;
        let tracking = &mut entry["delegation"];
        // The bot can acknowledge its delivery while a prompt refresh is still in flight.
        if tracking["state"] == "completed"
            && matches!(
                update,
                DelegationUpdate::Running { .. } | DelegationUpdate::Waiting { .. }
            )
        {
            return Ok(());
        }
        // Another recipient of the same room delegation can still be waiting for an answer.
        let prompt = match &update {
            DelegationUpdate::Waiting { chat_id, prompt_id } => {
                Some((chat_id.clone(), prompt_id.clone()))
            }
            DelegationUpdate::Completed => None,
            _ => self.prompts.pending_for_delegation(&link.entry_id)?,
        };
        let (state, detail) = match update {
            DelegationUpdate::Queued => ("queued", None),
            DelegationUpdate::Running { last_error } => ("running", last_error),
            DelegationUpdate::Waiting { .. } => ("waiting", None),
            DelegationUpdate::Completed => ("completed", None),
            DelegationUpdate::Failed(error) => ("failed", Some(error)),
        };
        tracking["state"] = if prompt.is_some() { "waiting" } else { state }.into();
        let (prompt_chat, prompt_id) = prompt.unzip();
        tracking["promptId"] = json!(prompt_id);
        tracking["promptChatId"] = json!(prompt_chat);
        tracking["detail"] = json!(detail);
        self.update_entry(&link.chat_id, &entry, events)
    }

    pub(crate) fn update_entry(
        &self,
        chat_id: &str,
        entry: &Value,
        events: &mut Events,
    ) -> Result<()> {
        self.transcript.update(chat_id, entry)?;
        if crate::domain::visible_to(&self.chats.get(chat_id)?, None) {
            events.push((
                "transcript.entryUpdated",
                json!({"chatId":chat_id,"entry":entry}),
            ));
        }
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
