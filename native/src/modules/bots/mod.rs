mod connection;
mod service;
mod validation;

pub(crate) use connection::{connect_bot, connect_for_turn};
pub(crate) use service::{adoptable_bots, create_bot, focus_bot, list_models, relaunch_bot};
