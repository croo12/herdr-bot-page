mod profile;
mod service;

pub(crate) use profile::{profile_for_pane, update_profile, ProfileTarget, ProfileUpdate};
pub(crate) use service::{edit_chat, find_chats, ChatEdit};
