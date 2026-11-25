use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use herdr_engine::{default_home, ipc, Engine};
use serde_json::{json, Value as V};
use std::{env, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "herdr-bot",
    version,
    about = "Local chat with agents running in herdr"
)]
struct Cli {
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Status,
    Serve,
    Rooms,
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
    Say(Message),
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
struct Message {
    chat: String,
    #[arg(required=true,num_args=1..,allow_hyphen_values=true)]
    text: Vec<String>,
}
#[derive(Subcommand)]
enum Bot {
    List,
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
enum Room {
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
enum Profile {
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

fn run() -> Result<()> {
    let cli = Cli::parse();
    let home = cli.home.unwrap_or_else(default_home);
    let pane = env::var("HERDR_PANE_ID").unwrap_or_default();
    let call = |m: &str, p: V| ipc::request(&home, m, p);
    let message = |m: &str, args: Message| {
        call(
            m,
            json!({"chatId":args.chat,"text":args.text.join(" "),"paneId":pane}),
        )
    };
    let value = match cli.command {
        Action::Serve => {
            let _engine = Engine::start(home, &env::current_exe()?, |_| {})?;
            eprintln!("herdr-bot host ready");
            loop {
                std::thread::park()
            }
        }
        Action::Status => call("host.status", V::Null)?,
        Action::Send(m) => message("message.send", m)?,
        Action::Say(m) => message("cli.say", m)?,
        Action::Message(m) => message("cli.message", m)?,
        Action::Pass { chat } => call("cli.pass", json!({"chatId":chat,"paneId":pane}))?,
        Action::Whoami => call("cli.whoami", json!({"paneId":pane}))?,
        Action::Read { chat, limit } => {
            let res = call("transcript.page", json!({"chatId":chat,"limit":limit}))?;
            for e in res["entries"].as_array().into_iter().flatten() {
                println!(
                    "{}: {}",
                    e["author"]["name"].as_str().unwrap_or("herdr-bot"),
                    e["content"]
                        .as_str()
                        .or(e["message"]["content"].as_str())
                        .or(e["prompt"]["question"].as_str())
                        .unwrap_or("")
                )
            }
            return Ok(());
        }
        Action::Bot { command: Bot::List } | Action::Rooms => {
            let kind = if matches!(cli.command, Action::Rooms) {
                "room"
            } else {
                "bot"
            };
            json!(call("chat.list", V::Null)?
                .as_array()
                .into_iter()
                .flatten()
                .filter(|x| x["kind"] == kind)
                .collect::<Vec<_>>())
        }
        Action::Bot { command } => match command {
            Bot::Adoptable => call("bot.listAdoptable", V::Null)?,
            Bot::Delete { id } => call("chat.delete", json!({"chatIds":[id]}))?,
            Bot::Adopt { pane, id, name } => call(
                "bot.create",
                json!({"id":id,"name":name.as_ref().unwrap_or(&id),"adoptPaneId":pane}),
            )?,
            Bot::Create {
                id,
                name,
                cwd,
                kind,
                model,
                reasoning,
                permission,
                description,
                allow_home_cwd,
            } => {
                let cwd = std::path::absolute(cwd)?;
                call(
                    "bot.create",
                    json!({"id":id,"name":name.as_ref().unwrap_or(&id),"cwd":cwd,"provider":kind,"model":model,"reasoningEffort":reasoning,
                    "permissionMode":permission,"description":description.unwrap_or_default(),"allowHomeCwd":allow_home_cwd}),
                )?
            }
            Bot::List => unreachable!(),
        },
        Action::Room { command } => match command {
            Room::Create {
                name,
                members,
                description,
            } => call(
                "room.create",
                json!({"name":name,"memberIds":members,"description":description}),
            )?,
            Room::Members { id, members } => {
                call("room.setMembers", json!({"roomId":id,"memberIds":members}))?
            }
        },
        Action::Profile {
            command: Profile::Get,
        } => call("cli.profile", json!({"paneId":pane}))?,
        Action::Profile {
            command:
                Profile::Update {
                    name,
                    label,
                    description,
                },
        } => {
            let mut p = json!({"paneId":pane});
            if name.is_none() && label.is_none() && description.is_none() {
                bail!("specify --name, --label or --description")
            }
            for (k, v) in [
                ("name", name),
                ("title", label),
                ("description", description),
            ] {
                if let Some(v) = v {
                    p[k] = v.into();
                }
            }
            call("cli.profileUpdate", p)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("herdr-bot: {e:#}");
        std::process::exit(1);
    }
}
