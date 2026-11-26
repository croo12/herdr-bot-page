use crate::modules::{bots, chats, prompts, rooms, settings, transcript, turns};
pub mod ipc;
use crate::{
    domain::{need, string_field},
    infrastructure::paths,
    modules::{chats::ChatEdit, turns::AgentAction},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy)]
enum Source {
    Desktop,
    Ipc,
}

impl Engine {
    pub fn call(self: &Arc<Self>, method: &str, params: Value) -> Result<Value> {
        self.call_from(Source::Desktop, method, params)
    }

    fn call_from(self: &Arc<Self>, source: Source, method: &str, params: Value) -> Result<Value> {
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

    fn dispatch(self: &Arc<Self>, source: Source, method: &str, params: Value) -> Result<Value> {
        if matches!(source, Source::Ipc) && method == "prompt.answer" {
            bail!("invalid_args: this action requires the desktop app");
        }
        match method {
            "identity.get" => Ok(json!({"userName":self.user})),
            "host.status" => self.host_status(),
            "directory.list" => paths::directory(string_field(&params, "path")),
            "settings.get" => settings::get_settings(self),
            "settings.update" => settings::update_settings(self, &params),
            "bot.defaults" => settings::bot_defaults(self),
            "chat.list" | "chat.search" => chats::find_chats(self, string_field(&params, "query")),
            "chat.updateProfile" => chats::edit_chat(self, ChatEdit::Profile, &params),
            "chat.setHidden" => chats::edit_chat(self, ChatEdit::Hidden, &params),
            "chat.setNotifyOnUpdates" => chats::edit_chat(self, ChatEdit::Notifications, &params),
            "chat.markRead" => chats::edit_chat(self, ChatEdit::Read, &params),
            "chat.setUnread" => chats::edit_chat(self, ChatEdit::Unread, &params),
            "chat.delete" => chats::delete_chats(self, &params),
            "room.create" => rooms::create_room(self, &params),
            "room.setMembers" => rooms::set_members(self, &params),
            "transcript.page" => transcript::transcript_page(self, &params),
            "transcript.thread" => transcript::transcript_thread(self, &params),
            "message.search" => transcript::search_messages(self, string_field(&params, "query")),
            "message.react" => transcript::react(self, &params),
            "message.send" => transcript::send(self, &params),
            "bot.listModels" => bots::list_models(need(&params, "provider")?),
            "bot.create" | "bot.quickCreate" => {
                bots::create_bot(self, params, method == "bot.quickCreate")
            }
            "bot.retrySetup" | "bot.updateLaunchSettings" => bots::relaunch_bot(self, &params),
            "bot.listAdoptable" => bots::adoptable_bots(self),
            "bot.focus" => bots::focus_bot(self, &params),
            "prompt.answer" => prompts::answer(self, &params),
            "cli.say" => turns::say(self, params),
            "cli.pass" => turns::agent_action(self, AgentAction::Pass, params),
            "cli.message" => turns::agent_action(self, AgentAction::Delegate, params),
            "cli.whoami" | "cli.profile" => turns::agent_action(self, AgentAction::Profile, params),
            "cli.profileUpdate" => turns::agent_action(self, AgentAction::UpdateProfile, params),
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
