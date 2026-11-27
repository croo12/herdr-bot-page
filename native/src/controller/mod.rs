use crate::modules::{bots, chats, prompts, rooms, settings, tasks, transcript, turns};
pub mod ipc;
use crate::{
    domain::{need, string_field, ChatJob},
    infrastructure::{paths, request_log::RequestLog, AgentSessions},
    modules::{chats::ChatEdit, turns::AgentAction},
    storage::Store,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) enum Source {
    Desktop,
    Ipc,
}

pub(crate) struct Controller<'a> {
    pub store: &'a Store,
    pub sessions: &'a AgentSessions,
    pub user: &'a str,
    pub request_log: &'a RequestLog,
    pub enqueue_chat: &'a dyn Fn(&str, ChatJob),
    pub host_status: &'a dyn Fn() -> Result<Value>,
}
impl Controller<'_> {
    pub(crate) fn call_from(&self, source: Source, method: &str, params: Value) -> Result<Value> {
        let start = Instant::now();
        let result = self.dispatch(source, method, params);
        let elapsed = start.elapsed();
        let source = match source {
            Source::Desktop => "desktop",
            Source::Ipc => "ipc",
        };
        let message = result.as_ref().err().map(|error| format!("{error:#}"));
        let code = message.as_deref().map(error_code);
        // Unknown method names can contain arbitrary input.
        let method = if code == Some("unknown_method") {
            "<unknown>"
        } else {
            method
        };
        self.request_log.record(source, method, elapsed, code);
        result
    }

    fn agent_action(&self, action: AgentAction, params: Value) -> Result<Value> {
        turns::agent_action(self.store, self.sessions, self.enqueue_chat, action, params)
    }

    fn dispatch(&self, source: Source, method: &str, params: Value) -> Result<Value> {
        if matches!(source, Source::Ipc)
            && matches!(
                method,
                "prompt.answer"
                    | "task.track"
                    | "task.assign"
                    | "task.setStatus"
                    | "task.run"
                    | "task.pause"
            )
        {
            bail!("invalid_args: this action requires the desktop app");
        }
        match method {
            "identity.get" => Ok(json!({"userName":self.user})),
            "host.status" => (self.host_status)(),
            "directory.list" => paths::directory(string_field(&params, "path")),
            "settings.get" => settings::get_settings(self.store),
            "settings.update" => settings::update_settings(self.store, &params),
            "bot.defaults" => bots::bot_defaults(self.store),
            "chat.openDirect" => self.store.tx(|repos, events| {
                let peer = if let Some(pane) = params["paneId"]
                    .as_str()
                    .filter(|pane_id| !pane_id.is_empty())
                {
                    Some(repos.chats.by_pane(pane)?)
                } else {
                    None
                };
                repos.chats.open_direct(
                    need(&params, "botId")?,
                    peer.as_ref().and_then(|bot| bot["id"].as_str()),
                    events,
                )
            }),
            "cli.inbox" => self.store.tx(|repos, _| {
                let bot = repos.chats.by_pane(need(&params, "paneId")?)?;
                let id = need(&bot, "id")?;
                if let Some(message) = params["ack"].as_str() {
                    repos.inbox.resolve(id, message, false)?;
                }
                if let Some(message) = params["retry"].as_str() {
                    repos.inbox.resolve(id, message, true)?;
                }
                Ok(json!(repos.inbox.list(id)?))
            }),
            "chat.list" | "chat.search" => {
                chats::find_chats(self.store, string_field(&params, "query"))
            }
            "chat.updateProfile" => chats::edit_chat(self.store, ChatEdit::Profile, &params),
            "chat.setHidden" => chats::edit_chat(self.store, ChatEdit::Hidden, &params),
            "chat.setNotifyOnUpdates" => {
                chats::edit_chat(self.store, ChatEdit::Notifications, &params)
            }
            "chat.markRead" => chats::edit_chat(self.store, ChatEdit::Read, &params),
            "chat.setUnread" => chats::edit_chat(self.store, ChatEdit::Unread, &params),
            "chat.delete" => crate::application::delete_chats(self.store, self.sessions, &params),
            "room.create" => rooms::create_room(self.store, &params),
            "room.setMembers" => {
                crate::application::set_members(self.store, self.sessions, &params)
            }
            "transcript.page" => transcript::transcript_page(self.store, &params),
            "transcript.thread" => transcript::transcript_thread(self.store, &params),
            "message.search" => {
                transcript::search_messages(self.store, string_field(&params, "query"))
            }
            "message.react" => transcript::react(self.store, &params),
            "task.track" => tasks::track(self.store, &params),
            "task.assign" => tasks::assign(self.store, &params),
            "task.setStatus" => tasks::set_status(self.store, &params),
            "task.list" => tasks::list(self.store, &params),
            "task.run" | "task.pause" => {
                tasks::control(self.store, &params, method == "task.pause")
            }
            "cli.taskReport" => tasks::report(self.store, &params),
            "message.send" => transcript::send(self.store, self.user, self.enqueue_chat, &params),
            "bot.listModels" => bots::list_models(need(&params, "provider")?),
            "bot.create" | "bot.quickCreate" => bots::create_bot(
                self.store,
                self.sessions,
                self.enqueue_chat,
                params,
                method == "bot.quickCreate",
            ),
            "bot.retrySetup" | "bot.updateLaunchSettings" => {
                bots::relaunch_bot(self.store, self.sessions, &params)
            }
            "bot.listAdoptable" => bots::adoptable_bots(self.store, self.sessions),
            "bot.focus" => bots::focus_bot(self.store, &self.sessions.herdr, &params),
            "bot.connect" => bots::connect_bot(self.store, self.sessions, &params),
            "prompt.answer" => prompts::answer(self.store, self.sessions, self.user, &params),
            "cli.say" => crate::application::post_reply(
                self.store,
                self.sessions,
                self.user,
                self.enqueue_chat,
                params,
            ),
            "cli.pass" => self.agent_action(AgentAction::Pass, params),
            "cli.message" => self.agent_action(AgentAction::Delegate, params),
            "cli.dm" => self.agent_action(AgentAction::Direct, params),
            "cli.whoami" | "cli.profile" => chats::profile_for_pane(self.store, &params),
            "cli.profileUpdate" => chats::update_profile_for_pane(self.store, params),
            _ => bail!("unknown_method: {method}"),
        }
    }
}
pub fn wire_error(error: &anyhow::Error) -> Value {
    let message = format!("{error:#}");
    json!({"code":error_code(&message),"message":message})
}

fn error_code(message: &str) -> &str {
    message
        .split_once(':')
        .map(|(code, _)| code)
        .filter(|code| {
            code.chars()
                .all(|character| character.is_ascii_lowercase() || character == '_')
        })
        .unwrap_or("internal")
}
