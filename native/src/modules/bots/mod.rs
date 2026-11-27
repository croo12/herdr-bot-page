mod connection;
mod service;
mod working_directory;

pub(crate) use connection::{connect_bot, connect_for_turn};
pub(crate) use service::{
    adoptable_bots, bot_defaults, create_bot, focus_bot, list_models, relaunch_bot,
};
