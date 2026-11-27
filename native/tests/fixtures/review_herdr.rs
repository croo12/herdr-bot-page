use std::{env, fs, path::PathBuf, time::Duration};

fn main() {
    let args: Vec<_> = env::args().skip(3).collect();
    let home = PathBuf::from(env::var_os("HERDR_BOT_HOME").unwrap());
    let blocked = home.join("blocked");
    let status = if blocked.exists() { "blocked" } else { "idle" };
    let worker = format!(
        r#"{{"name":"worker","pane_id":"p1","agent":"codex","agent_status":"{status}","cwd":"."}}"#
    );
    let peer_status = if home.join("reviewer-finishing").exists() { "working" } else { "idle" };
    let reviewer = format!(r#"{{"name":"peer","pane_id":"p2","agent":"codex","agent_status":"{peer_status}","cwd":"."}}"#);
    match (args[0].as_str(),args[1].as_str()) {
        ("session","list") => println!(r#"{{"sessions":[{{"name":"review-test","running":true}}]}}"#),
        ("agent","list") => println!(r#"{{"result":{{"agents":[{worker},{reviewer}]}}}}"#),
        ("agent","get") => println!(r#"{{"result":{{"agent":{}}}}}"#,if args[2]=="p1" {&worker} else {&reviewer}),
        ("agent","read") => println!("Would you like to run the following command?\n$ cargo test {}\n› 1. Yes, proceed (y)\n  2. Yes, always allow (p)\n  3. No (esc)\nPress enter to confirm or esc to cancel",fs::read_to_string(&blocked).unwrap()),
        ("agent","prompt") if args[2]=="p1" && args[3].contains("The permission request was denied") => {
            if home.join("reviewer-finishing").exists() {
                assert!(args[3].contains("Use the test worktree"));
                fs::File::create_new(home.join("denial-follow-up")).unwrap();
            }
            let room = fs::read_to_string(home.join("room")).unwrap();
            let cli = home.join("bin").join(format!("herdr-bot{}",env::consts::EXE_SUFFIX));
            assert!(std::process::Command::new(cli).env("HERDR_PANE_ID","p1")
                .args(["say",&room,"I will request permission for the corrected worktree."]).output().unwrap().status.success());
            println!(r#"{{"result":{{"agent":{worker}}}}}"#);
        }
        ("agent","prompt") if args[2]=="p1" => {
            if home.join("delegation-failure").exists() {
                eprintln!(r#"{{"error":{{"code":"agent_unavailable","message":"test worker is unavailable"}}}}"#);
                std::process::exit(1);
            }
            if home.join("delegation-room").exists() {
                let chat = args[3].split_once("arguments: say ").unwrap().1.split_whitespace().next().unwrap();
                fs::write(home.join("delegation-target"), chat).unwrap();
            }
            fs::write(&blocked,"--lib").unwrap();
            println!(r#"{{"result":{{"agent":{}}}}}"#,worker.replace("idle","blocked"));
        }
        ("agent","wait") => {
            for _ in 0..1200 {
                if !blocked.exists() { break; }
                std::thread::sleep(Duration::from_millis(25));
            }
            if home.join("delegation-room").exists() {
                let cli = home.join("bin").join(format!("herdr-bot{}", env::consts::EXE_SUFFIX));
                assert!(std::process::Command::new(cli).env("HERDR_PANE_ID","p1")
                    .args(["say",&fs::read_to_string(home.join("delegation-target")).unwrap(),"Verified the delegated build"]).output().unwrap().status.success());
            }
            println!(r#"{{"result":{{"agent":{}}}}}"#,worker.replace("blocked","done"));
        }
        ("agent","prompt") if args[2]=="p2" => {
            assert_eq!(peer_status,"idle");
            if home.join("delegation-room").exists() {
                let chat = args[3].split_once("arguments: say ").unwrap().1.split_whitespace().next().unwrap();
                let cli = home.join("bin").join(format!("herdr-bot{}", env::consts::EXE_SUFFIX));
                let mut command = std::process::Command::new(cli);
                command.env("HERDR_PANE_ID","p2");
                let marker = if home.join("delegation-failure").exists() { "delegated-again" } else { "delegated-once" };
                if fs::read_to_string(home.join("delegation-room")).unwrap() == chat && fs::File::create_new(home.join(marker)).is_ok() {
                    command.args(["message","worker","Check the delegated build"]);
                } else {
                    command.args(["pass",chat]);
                }
                assert!(command.output().unwrap().status.success());
            }
            if let Some((_,args)) = args[3].split_once("arguments: say --reply-to ") {
                fs::write(home.join("ticket"),args.split_whitespace().next().unwrap()).unwrap();
            }
            println!(r#"{{"result":{{"agent":{reviewer}}}}}"#);
        }
        ("agent","send-keys") => {
            assert_eq!(args[2],"p1");
            fs::write(home.join("key"),&args[3]).unwrap();
            fs::remove_file(blocked).unwrap();
            println!(r#"{{"result":{{}}}}"#);
        }
        _ => println!(r#"{{"result":{{}}}}"#),
    }
}
