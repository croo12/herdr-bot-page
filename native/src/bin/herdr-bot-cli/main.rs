mod args;
use anyhow::{bail, Result};
use args::{Action, Bot, Cli, Message, Profile, Room};
use clap::Parser;
use herdr_engine::{default_home, ipc, Engine};
use serde_json::{json, Value};
use std::env;
fn run() -> Result<()> {
    let cli = Cli::parse();
    let home = cli.home.unwrap_or_else(default_home);
    let pane = env::var("HERDR_PANE_ID").unwrap_or_default();
    let call = |method: &str, params: Value| ipc::request(&home, method, params);
    let message = |method: &str, args: Message| {
        call(
            method,
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
        Action::Status => call("host.status", Value::Null)?,
        Action::Send(message_args) => message("message.send", message_args)?,
        Action::Say {
            reply_to,
            decision,
            message: message_args,
        } => call(
            "cli.say",
            json!({"paneId":pane,"chatId":message_args.chat,"text":message_args.text.join(" "),"replyTo":reply_to,"decision":decision}),
        )?,
        Action::Message(message_args) => message("cli.message", message_args)?,
        Action::Pass { chat } => call("cli.pass", json!({"chatId":chat,"paneId":pane}))?,
        Action::Whoami => call("cli.whoami", json!({"paneId":pane}))?,
        Action::Read { chat, limit } => {
            let res = call("transcript.page", json!({"chatId":chat,"limit":limit}))?;
            for entry in res["entries"].as_array().into_iter().flatten() {
                println!(
                    "{}: {}",
                    entry["author"]["name"].as_str().unwrap_or("herdr-bot"),
                    entry["content"]
                        .as_str()
                        .or(entry["message"]["content"].as_str())
                        .or(entry["prompt"]["question"].as_str())
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
            json!(call("chat.list", Value::Null)?
                .as_array()
                .into_iter()
                .flatten()
                .filter(|chat| chat["kind"] == kind)
                .collect::<Vec<_>>())
        }
        Action::Bot { command } => match command {
            Bot::Adoptable => call("bot.listAdoptable", Value::Null)?,
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
            let mut params = json!({"paneId":pane});
            if name.is_none() && label.is_none() && description.is_none() {
                bail!("specify --name, --label or --description")
            }
            for (field, value) in [
                ("name", name),
                ("title", label),
                ("description", description),
            ] {
                if let Some(value) = value {
                    params[field] = value.into();
                }
            }
            call("cli.profileUpdate", params)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("herdr-bot: {error:#}");
        std::process::exit(1);
    }
}
