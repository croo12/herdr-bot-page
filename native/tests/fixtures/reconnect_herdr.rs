use std::{env, fs, path::PathBuf, process::Command};

fn agent(name: &str, pane: &str) -> String {
    let provider = match name {
        "managed" => "codex",
        "external" => "claude",
        _ => "pi",
    };
    format!(
        r#"{{"name":"{name}","pane_id":"{pane}","agent":"{provider}","agent_status":"idle","cwd":"."}}"#
    )
}

fn main() {
    let args: Vec<String> = env::args().skip(3).collect();
    let home = PathBuf::from(env::var_os("HERDR_BOT_HOME").unwrap());
    let reset = home.join("reset").exists();
    let managed = if reset { "w8:p1" } else { "w5:p1" };
    let moved = if reset { "w8:p2" } else { "w5:p2" };
    let fields = match (args[0].as_str(), args[1].as_str()) {
        ("session", "list") => {
            println!(r#"{{"sessions":[{{"name":"reconnect-test","running":true}}]}}"#);
            return;
        }
        ("agent", "list") => {
            let mut rows = vec![agent("moved", moved)];
            if !home.join("external-closed").exists() {
                rows.push(agent("external", "w5:p3"));
            }
            if home.join(managed.replace(':', "-")).exists() {
                rows.push(agent("managed", managed));
            }
            if reset {
                rows.push(agent("unrelated", "w5:p1"));
            }
            format!(r#""agents":[{}]"#, rows.join(","))
        }
        ("workspace", "list") => r#""workspaces":[]"#.into(),
        ("workspace", "create") => {
            let pane = if PathBuf::from(&args[3]) == home.join("relaunch") { "w9:p1" } else { managed };
            format!(r#""root_pane":{{"pane_id":"{pane}"}}"#)
        }
        ("agent", "start") if args[6] == "w9:p1" => {
            assert_eq!(args[4], "claude");
            fs::write(home.join("last-launch-options"), args[9..].join(" ")).unwrap();
            format!(r#""agent":{}"#, agent("external", "w9:p1"))
        }
        ("agent", "start") => {
            assert_eq!(args[2], "managed");
            assert_eq!(args[6], managed);
            assert_eq!(args[4], "codex");
            assert_eq!(&args[9..], ["--", "-s", "workspace-write", "-a", "on-request", "--model", "test-model", "--config", "model_reasoning_effort=\"high\""]);
            fs::File::create_new(home.join(managed.replace(':', "-"))).unwrap();
            format!(r#""agent":{}"#, agent("managed", managed))
        }
        ("agent", "rename") if home.join("fail-replacement").exists() => {
            match (args[2].as_str(), args[3].as_str()) {
                ("w5:p3", "--clear") => fs::write(home.join("name-cleared"), "").unwrap(),
                ("w9:p1", "external") => {
                    assert!(home.join("name-cleared").exists());
                    eprintln!("simulated rename failure");
                    std::process::exit(1);
                }
                ("w5:p3", "external") => fs::write(home.join("name-restored"), "").unwrap(),
                _ => panic!("unexpected rename {:?}", args),
            }
            String::new()
        }
        ("pane", "close") if args[2] == "w9:p1" => {
            fs::write(home.join("replacement-closed"), "").unwrap();
            String::new()
        }
        ("agent", "rename") => String::new(),
        ("agent", "prompt") => {
            let pane = args[2].as_str();
            assert!(reset);
            let bot = if pane == moved {
                "moved"
            } else {
                assert_eq!(pane, managed);
                "managed"
            };
            let room = args[3]
                .split_once("in herdr-bot chat ")
                .unwrap()
                .1
                .split('.')
                .next()
                .unwrap();
            if bot == "managed" {
                assert!(args[3].starts_with(&format!(
                    "You are Managed (bot ID managed) in herdr-bot chat {room}. Role: Check the UI for accessibility. Members:"
                )));
            }
            let cli = home
                .join("bin")
                .join(format!("herdr-bot{}", env::consts::EXE_SUFFIX));
            let args = if fs::File::create_new(home.join(format!("{bot}-replied"))).is_ok() {
                vec!["say", room, bot]
            } else {
                vec!["pass", room]
            };
            assert!(Command::new(cli)
                .args(args)
                .env("HERDR_PANE_ID", pane)
                .output()
                .unwrap()
                .status
                .success());
            format!(r#""agent":{}"#, agent(bot, pane))
        }
        _ => panic!("unexpected operation {:?}", args),
    };
    println!(r#"{{"result":{{{fields}}}}}"#);
}
