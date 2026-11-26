use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    name = "herdr-bot",
    version,
    about = "Local chat with agents running in herdr"
)]
pub(super) struct Cli {
    #[arg(long, global = true)]
    pub(super) home: Option<PathBuf>,
    #[command(subcommand)]
    pub(super) command: Action,
}
#[derive(Subcommand)]
pub(super) enum Action {
    Status,
    Serve,
    Rooms,
    /// List tasks recorded on messages in a room.
    Tasks {
        room: String,
    },
    /// Post a checkpoint or verified result from the assigned pane.
    TaskReport {
        room: String,
        entry_id: String,
        attempt_id: String,
        #[arg(value_parser = ["completed", "continue", "blocked"])]
        outcome: String,
        #[arg(required = true, num_args = 1..)]
        summary: Vec<String>,
    },
    Whoami,
    Bot {
        #[command(subcommand)]
        command: Bot,
    },
    Room {
        #[command(subcommand)]
        command: Room,
    },
    Profile {
        #[command(subcommand)]
        command: Profile,
    },
    Send(Message),
    Say {
        #[arg(long)]
        reply_to: Option<String>,
        #[arg(long, requires = "reply_to", value_parser = ["approve", "deny", "human"])]
        decision: Option<String>,
        #[command(flatten)]
        message: Message,
    },
    Message(Message),
    Pass {
        chat: String,
    },
    Read {
        chat: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
}
#[derive(Args)]
pub(super) struct Message {
    pub(super) chat: String,
    #[arg(required=true,num_args=1..,allow_hyphen_values=true)]
    pub(super) text: Vec<String>,
}
#[derive(Subcommand)]
pub(super) enum Bot {
    List,
    /// Reconnect a bot without sending a message or interrupting a live pane.
    Connect {
        id: String,
    },
    Adoptable,
    Delete {
        id: String,
    },
    Adopt {
        pane: String,
        id: String,
        #[arg(long)]
        name: Option<String>,
    },
    Create {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: PathBuf,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        reasoning: Option<String>,
        #[arg(long, alias = "permission-mode", default_value = "ask")]
        permission: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        allow_home_cwd: bool,
    },
}
#[derive(Subcommand)]
pub(super) enum Room {
    Create {
        name: String,
        #[arg(long, value_delimiter = ',')]
        members: Vec<String>,
        #[arg(long, default_value = "")]
        description: String,
    },
    Members {
        id: String,
        #[arg(value_delimiter = ',')]
        members: Vec<String>,
    },
}
#[derive(Subcommand)]
pub(super) enum Profile {
    Get,
    Update {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
}
