use super::Engine;
use crate::{
    application,
    domain::ChatJob,
    modules::{rooms, tasks},
};
use anyhow::Result;
use std::{sync::Arc, thread};

pub(super) enum BotWork {
    Inbox,
    Greeting { chat: String, job: ChatJob },
    Task { chat: String, entry: String },
}

impl Engine {
    pub(super) fn wake_bot(self: &Arc<Self>, bot: &str) {
        self.enqueue(bot, BotWork::Inbox);
    }

    pub(super) fn recover_work(self: &Arc<Self>) -> Result<()> {
        for bot in self.store.tx(|repos, _| repos.inbox.ready_bots())? {
            self.wake_bot(&bot);
        }
        for (chat, entry, assignee) in tasks::runnable(&self.store)? {
            self.enqueue(&assignee, BotWork::Task { chat, entry });
        }
        Ok(())
    }

    pub(super) fn enqueue(self: &Arc<Self>, bot: &str, work: BotWork) {
        let mut queues = self.bot_queues.lock().unwrap();
        let start_worker = !queues.contains_key(bot);
        let queue = queues.entry(bot.into()).or_default();
        if queue.iter().any(|waiting| match (&work, waiting) {
            (BotWork::Inbox, BotWork::Inbox) => true,
            (
                BotWork::Task { chat, entry },
                BotWork::Task {
                    chat: other_chat,
                    entry: other_entry,
                },
            ) => chat == other_chat && entry == other_entry,
            _ => false,
        }) {
            return;
        }
        queue.push_back(work);
        drop(queues);
        if start_worker {
            let engine = Arc::clone(self);
            let bot = bot.to_owned();
            thread::spawn(move || engine.drain_bot(&bot));
        }
    }

    fn drain_bot(self: &Arc<Self>, bot: &str) {
        loop {
            let work = {
                let mut queues = self.bot_queues.lock().unwrap();
                let Some(work) = queues.get_mut(bot).and_then(|queue| queue.pop_front()) else {
                    queues.remove(bot);
                    return;
                };
                work
            };
            match work {
                BotWork::Inbox => match self.store.tx(|repos, _| repos.inbox.reserve_next(bot)) {
                    Ok(Some((chat, job))) => {
                        self.in_chat(&chat, || self.run_chat_job(&chat, &job));
                        // Other queued work gets a turn before claiming the next message.
                        self.wake_bot(bot);
                    }
                    Ok(None) => {}
                    Err(error) => eprintln!("Could not read inbox for {bot}: {error:#}"),
                },
                BotWork::Greeting { chat, job } => {
                    self.in_chat(&chat, || self.run_chat_job(&chat, &job))
                }
                BotWork::Task { chat, entry } => self.in_chat(&chat, || {
                    application::execute_task(&self.store, &self.sessions, &chat, &entry, bot)
                }),
            }
        }
    }

    fn run_chat_job(&self, chat: &str, job: &ChatJob) -> Result<()> {
        let result = application::run_chat(&self.store, &self.sessions, chat, job);
        let Some(delivery) = &job.delivery else {
            return result;
        };
        let message = result.as_ref().err().map(|error| format!("{error:#}"));
        let report = self.store.tx(|repos, events| {
            let report = repos.inbox.release(delivery, message.as_deref())?;
            if message.is_some() {
                repos.sync_delivery_state(delivery, job.context.delegation.as_ref(), events)?;
            }
            Ok(report)
        })?;
        if report {
            result
        } else {
            Ok(())
        }
    }

    fn in_chat(&self, chat: &str, run: impl FnOnce() -> Result<()>) {
        {
            let mut active = self.active_chats.lock().unwrap();
            let workers = active.entry(chat.into()).or_default();
            *workers += 1;
            if *workers == 1 {
                rooms::set_activity(&self.store, chat, true);
            }
        }
        if let Err(error) = run() {
            self.store.notice(chat, &format!("{error:#}"));
        }
        let mut active = self.active_chats.lock().unwrap();
        let workers = active.get_mut(chat).expect("chat activity was registered");
        *workers -= 1;
        if *workers == 0 {
            active.remove(chat);
            rooms::set_activity(&self.store, chat, false);
        }
    }
}
