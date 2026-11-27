use crate::{
    domain::{
        bot::{AvatarColor, Bot, BotId},
        need,
    },
    storage::Store,
};
use anyhow::Result;
use serde_json::{json, Value};

pub(crate) fn profile_for_pane(store: &Store, params: &Value) -> Result<Value> {
    let pane = need(params, "paneId")?;
    store.tx(|repos, _| repos.chats.by_pane(pane))
}

pub(crate) struct ProfileUpdate {
    pub name: Option<String>,
    pub label: Option<String>,
    pub instruction: Option<String>,
    pub avatar_color: Option<Option<AvatarColor>>,
    pub avatar_shape: Option<Option<String>>,
}

impl ProfileUpdate {
    fn apply_to_bot(self, bot: &mut Bot) {
        if let Some(name) = self.name {
            bot.name = name;
        }
        if let Some(label) = self.label {
            bot.label = label;
        }
        if let Some(instruction) = self.instruction {
            bot.instruction = instruction;
        }
        if let Some(color) = self.avatar_color {
            bot.avatar_color = color;
        }
        if let Some(shape) = self.avatar_shape {
            bot.avatar_shape = shape;
        }
    }

    fn apply_to_chat(self, chat: &mut Value) {
        if let Some(name) = self.name {
            chat["name"] = name.into();
        }
        if let Some(label) = self.label {
            chat["title"] = label.into();
        }
        if let Some(instruction) = self.instruction {
            chat["description"] = instruction.into();
        }
        if let Some(color) = self.avatar_color {
            chat["avatarColor"] = json!(color);
        }
        if let Some(shape) = self.avatar_shape {
            chat["avatarShape"] = json!(shape);
        }
    }
}

pub(crate) enum ProfileTarget<'a> {
    Chat(&'a str),
    Pane(&'a str),
}

pub(crate) fn update_profile(
    store: &Store,
    target: ProfileTarget,
    input: ProfileUpdate,
) -> Result<Value> {
    store.tx(|repos, events| {
        let id = match target {
            ProfileTarget::Pane(pane) => repos.bots.require_pane_owner(pane)?.as_str().to_owned(),
            ProfileTarget::Chat(id) => id.to_owned(),
        };
        let mut chat = repos.chats.get(&id)?;
        anyhow::ensure!(
            crate::domain::visible_to(&chat, None),
            "invalid_args: not a participant"
        );
        if chat["kind"] == "bot" {
            let mut bot = repos.bots.get(&BotId::from(id.as_str()))?;
            input.apply_to_bot(&mut bot);
            return repos.update_bot_profile(&bot, events);
        }
        input.apply_to_chat(&mut chat);
        repos.chats.save_changed(&chat, events)?;
        Ok(chat)
    })
}
