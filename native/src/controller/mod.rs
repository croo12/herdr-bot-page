use crate::modules::{bots, chats, prompts, rooms, settings, transcript, turns};
pub mod ipc;
use crate::{
    domain::{need, s},
    infrastructure::paths,
    modules::{chats::ChatEdit, turns::AgentAction},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy)]
enum Source {
    Desktop,
    Ipc,
}

impl Engine {
    pub fn call(self: &Arc<Self>, method: &str, p: V) -> Result<V> {
        self.call_from(Source::Desktop, method, p)
    }

    fn call_from(self: &Arc<Self>, source: Source, method: &str, p: V) -> Result<V> {
        let start = Instant::now();
        let result = self.dispatch(source, method, p);
        let elapsed = start.elapsed();
        let source = match source {
            Source::Desktop => "desktop",
            Source::Ipc => "ipc",
        };
        let message = result.as_ref().err().map(|e| format!("{e:#}"));
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

    fn dispatch(self: &Arc<Self>, source: Source, method: &str, p: V) -> Result<V> {
        if matches!(source, Source::Ipc) && method == "prompt.answer" {
            bail!("invalid_args: this action requires the desktop app");
        }
        match method {
            "identity.get" => Ok(json!({"userName":self.user})),
            "host.status" => self.host_status(),
            "directory.list" => paths::directory(s(&p, "path")),
            "settings.get" => settings::get_settings(self),
            "settings.update" => settings::update_settings(self, &p),
            "bot.defaults" => settings::bot_defaults(self),
            "chat.list" | "chat.search" => chats::find_chats(self, s(&p, "query")),
            "chat.updateProfile" => chats::edit_chat(self, ChatEdit::Profile, &p),
            "chat.setHidden" => chats::edit_chat(self, ChatEdit::Hidden, &p),
            "chat.setNotifyOnUpdates" => chats::edit_chat(self, ChatEdit::Notifications, &p),
            "chat.markRead" => chats::edit_chat(self, ChatEdit::Read, &p),
            "chat.setUnread" => chats::edit_chat(self, ChatEdit::Unread, &p),
            "chat.delete" => chats::delete_chats(self, &p),
            "room.create" => rooms::create_room(self, &p),
            "room.setMembers" => rooms::set_members(self, &p),
            "transcript.page" => transcript::transcript_page(self, &p),
            "transcript.thread" => transcript::transcript_thread(self, &p),
            "message.search" => transcript::search_messages(self, s(&p, "query")),
            "message.react" => transcript::react(self, &p),
            "message.send" => transcript::send(self, &p),
            "bot.listModels" => bots::list_models(need(&p, "provider")?),
            "bot.create" | "bot.quickCreate" => {
                bots::create_bot(self, p, method == "bot.quickCreate")
            }
            "bot.retrySetup" | "bot.updateLaunchSettings" => bots::relaunch_bot(self, &p),
            "bot.listAdoptable" => bots::adoptable_bots(self),
            "bot.focus" => bots::focus_bot(self, &p),
            "prompt.answer" => prompts::answer(self, &p),
            "cli.say" => turns::say(self, p),
            "cli.pass" => turns::agent_action(self, AgentAction::Pass, p),
            "cli.message" => turns::agent_action(self, AgentAction::Delegate, p),
            "cli.whoami" | "cli.profile" => turns::agent_action(self, AgentAction::Profile, p),
            "cli.profileUpdate" => turns::agent_action(self, AgentAction::UpdateProfile, p),
            _ => bail!("unknown_method: {method}"),
        }
    }
}
pub fn wire_error(e: &anyhow::Error) -> V {
    let message = format!("{e:#}");
    json!({"code":error_code(&message),"message":message})
}

fn error_code(message: &str) -> &str {
    message
        .split_once(':')
        .map(|(code, _)| code)
        .filter(|c| c.chars().all(|x| x.is_ascii_lowercase() || x == '_'))
        .unwrap_or("internal")
}
