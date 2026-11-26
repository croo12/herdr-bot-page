use std::{env, fs, path::PathBuf, time::Duration};

fn main() {
    let args: Vec<_> = env::args().skip(3).collect();
    let home = PathBuf::from(env::var_os("HERDR_BOT_HOME").unwrap());
    let blocked = home.join("blocked");
    let status = if blocked.exists() { "blocked" } else { "idle" };
    let worker = format!(
        r#"{{"name":"worker","pane_id":"p1","agent":"codex","agent_status":"{status}","cwd":"."}}"#
    );
    let reviewer =
        r#"{"name":"peer","pane_id":"p2","agent":"codex","agent_status":"idle","cwd":"."}"#;
    match (args[0].as_str(),args[1].as_str()) {
        ("session","list") => println!(r#"{{"sessions":[{{"name":"review-test","running":true}}]}}"#),
        ("agent","list") => println!(r#"{{"result":{{"agents":[{worker},{reviewer}]}}}}"#),
        ("agent","get") => println!(r#"{{"result":{{"agent":{}}}}}"#,if args[2]=="p1" {&worker} else {reviewer}),
        ("agent","read") => println!("Would you like to run the following command?\n$ cargo test {}\n› 1. Yes, proceed (y)\n  2. Yes, always allow (p)\n  3. No (esc)\nPress enter to confirm or esc to cancel",fs::read_to_string(&blocked).unwrap()),
        ("agent","prompt") if args[2]=="p1" => {
            fs::write(&blocked,"--lib").unwrap();
            println!(r#"{{"result":{{"agent":{}}}}}"#,worker.replace("idle","blocked"));
        }
        ("agent","wait") => {
            for _ in 0..1200 {
                if !blocked.exists() { break; }
                std::thread::sleep(Duration::from_millis(25));
            }
            println!(r#"{{"result":{{"agent":{}}}}}"#,worker.replace("blocked","done"));
        }
        ("agent","prompt") if args[2]=="p2" => {
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
