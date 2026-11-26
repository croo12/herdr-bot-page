use super::Engine;
use crate::{
    application,
    domain::ChatJob,
    modules::{rooms, tasks},
};
use anyhow::Result;
use std::{sync::Arc, thread};

pub(super) enum QueueItem {
    Chat(ChatJob),
    Task(String),
}

impl Engine {
    pub(super) fn enqueue_chat(self: &Arc<Self>, chat_id: &str, job: ChatJob) {
        self.enqueue(chat_id, QueueItem::Chat(job));
    }

    pub(super) fn enqueue_runnable_tasks(self: &Arc<Self>) -> Result<()> {
        for (room, entry_id) in tasks::runnable(&self.store)? {
            self.enqueue(&room, QueueItem::Task(entry_id));
        }
        Ok(())
    }

    fn enqueue(self: &Arc<Self>, chat_id: &str, item: QueueItem) {
        let mut queues = self.queues.lock().unwrap();
        let start_worker = !queues.contains_key(chat_id);
        let queue = queues.entry(chat_id.to_owned()).or_default();
        if let QueueItem::Task(entry_id) = &item {
            if queue
                .iter()
                .any(|queued| matches!(queued, QueueItem::Task(id) if id == entry_id))
            {
                return;
            }
        }
        queue.push_back(item);
        drop(queues);
        if start_worker {
            let engine = Arc::clone(self);
            let chat_id = chat_id.to_owned();
            thread::spawn(move || engine.drain_queue(&chat_id));
        }
    }

    fn drain_queue(&self, chat_id: &str) {
        loop {
            let item = {
                let mut queues = self.queues.lock().unwrap();
                match queues.get_mut(chat_id).and_then(|queue| queue.pop_front()) {
                    Some(item) => item,
                    None => {
                        queues.remove(chat_id);
                        return;
                    }
                }
            };
            rooms::set_activity(&self.store, chat_id, true);
            let result = match item {
                QueueItem::Chat(job) => {
                    application::run_chat(&self.store, &self.sessions, chat_id, job)
                }
                QueueItem::Task(entry_id) => {
                    application::execute_task(&self.store, &self.sessions, chat_id, &entry_id)
                }
            };
            if let Err(error) = result {
                self.store.notice(chat_id, &format!("{error:#}"));
            }
            rooms::set_activity(&self.store, chat_id, false);
        }
    }
}
