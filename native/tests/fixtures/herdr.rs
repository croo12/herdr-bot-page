// Stand-in for the external herdr process. A prompt invokes the real native CLI
// inside a simulated pane, just as an agent posts its answer on Windows.
use std::{env, fs, path::PathBuf, process::Command, time::Duration};
fn main() {
    let args: Vec<String> = env::args().skip(3).collect();
    let home = PathBuf::from(env::var_os("HERDR_BOT_HOME").unwrap());
    let stage = home.join("delayed-stage");
    let status = fs::read_to_string(&stage).unwrap_or_else(|_| "idle".to_owned());
    let agent = format!(
        r#"{{"pane_id":"w1:p1","name":"tester","agent":"pi","agent_status":"{status}","cwd":"."}}"#
    );
    match (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    ) {
        (Some("session"), Some("list")) => {
            println!(r#"{{"sessions":[{{"name":"native-test","running":true}}]}}"#)
        }
        (Some("agent"), Some("list")) => println!(r#"{{"result":{{"agents":[{agent}]}}}}"#),
        (Some("agent"), Some("get")) | (Some("agent"), Some("start")) => {
            println!(r#"{{"result":{{"agent":{agent}}}}}"#)
        }
        (Some("agent"), Some("prompt")) => {
            let prompt = &args[3];
            if prompt.contains("simulate delayed turn") {
                assert!(fs::File::create_new(home.join("submitted-once")).is_ok());
                fs::write(stage, "blocked").unwrap();
                eprintln!(
                    r#"{{"error":{{"code":"timeout","message":"timed out waiting for agent status"}},"id":"cli:agent:prompt"}}"#
                );
                std::process::exit(1);
            }
            let target = prompt
                .split_once("in herdr-bot chat ")
                .unwrap()
                .1
                .split('.')
                .next()
                .unwrap();
            let cli = PathBuf::from(env::var_os("HERDR_BOT_HOME").unwrap())
                .join("bin")
                .join(format!("herdr-bot{}", env::consts::EXE_SUFFIX));
            let mut invoke = Command::new(cli);
            invoke.env("HERDR_PANE_ID", "w1:p1");
            if prompt.contains("native-reply") && target.starts_with("room-") {
                invoke.args(["pass", target]);
            } else {
                invoke.args(["say", target, "native-reply 안녕하세요"]);
            }
            let out = invoke.output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            println!(r#"{{"result":{{"agent":{agent}}}}}"#)
        }
        (Some("agent"), Some("wait")) => {
            assert_eq!(&args[3..7], ["--until", "done", "--until", "idle"]);
            if status == "blocked" {
                fs::write(stage, "working").unwrap();
                println!(r#"{{"error":{{"code":"timeout","message":"wait timed out"}}}}"#);
            } else {
                assert_eq!(status, "working");
                fs::write(home.join("waiting-for-reply"), "").unwrap();
                for _ in 0..400 {
                    if home.join("reply-posted").exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                fs::remove_file(stage).unwrap();
                println!(
                    r#"{{"result":{{"agent":{}}}}}"#,
                    agent.replace("working", "done")
                );
            }
        }
        _ => println!(r#"{{"result":{{}}}}"#),
    }
}
