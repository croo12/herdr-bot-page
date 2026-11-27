// Stand-in for the external herdr process. A prompt invokes the real native CLI
// inside a simulated pane, just as an agent posts its answer on Windows.
use std::{env, fs, io::Write, path::PathBuf, process::Command, time::Duration};
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
        (Some("agent"), Some("list")) if home.join("two-bots").exists() => {
            let peer = agent.replace("tester", "peer").replace("w1:p1", "w1:p2");
            println!(r#"{{"result":{{"agents":[{agent},{peer}]}}}}"#);
        }
        (Some("agent"), Some("list")) => println!(r#"{{"result":{{"agents":[{agent}]}}}}"#),
        (Some("agent"), Some("get")) | (Some("agent"), Some("start")) => {
            let agent = if args[2] == "w1:p2" && home.join("two-bots").exists() {
                agent.replace("tester", "peer").replace("w1:p1", "w1:p2")
            } else { agent };
            println!(r#"{{"result":{{"agent":{agent}}}}}"#)
        }
        (Some("agent"), Some("prompt")) => {
            let prompt = &args[3];
            if prompt.contains("Current request:\nsimulate stalled turn\n") {
                fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(home.join("prompt-count"))
                    .unwrap()
                    .write_all(b"1")
                    .unwrap();
                fs::write(stage, "done").unwrap();
                eprintln!(
                    r#"{{"error":{{"code":"agent_prompt_stalled","message":"agent prompt produced no observed working or blocked state within 5000 ms; current status is done"}}}}"#
                );
                std::process::exit(1);
            }
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
            invoke.env("HERDR_PANE_ID", &args[2]);
            if home.join("bot-workers").exists() {
                let request = prompt.split_once("Current request:\n").unwrap().1.lines().next().unwrap();
                let reply = match (args[2].as_str(), request) {
                    ("w1:p1", "@tester build") => {
                        fs::File::create_new(home.join("build-started")).unwrap();
                        for _ in 0..800 {
                            if home.join("release-build").exists() { break; }
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        assert!(home.join("release-build").exists());
                        "Build complete"
                    }
                    ("w1:p2", "@peer review") => "Review complete",
                    ("w1:p1", "Follow up") => {
                        assert!(home.join("release-build").exists());
                        "Follow-up complete"
                    }
                    _ => panic!("Unexpected delivery to {}: {request}", args[2]),
                };
                invoke.args(["say", target, reply]);
            } else if home.join("room-replies").exists() {
                let request = prompt.split_once("Current request:\n").unwrap().1.lines().next().unwrap();
                let response = match (args[2].as_str(), request) {
                    ("w1:p1", "@tester check packaging") => "@peer find the failing step",
                    ("w1:p2", "@peer find the failing step") => "Signing failed",
                    ("w1:p1", "Signing failed") => "I will check signing",
                    _ => panic!("Unexpected delivery to {}: {request}", args[2]),
                };
                let parent = prompt.split_once("say --reply-to ").unwrap().1.split_whitespace().next().unwrap();
                invoke.args(["say", "--reply-to", parent, target, response]);
            } else if let Some(report) = prompt.lines().find(|line| line.starts_with("task-report ")) {
                assert!(prompt.contains("Completion criteria: Windows build passes"));
                if prompt.contains("Previous checkpoint: Build fixed; verify on Windows") {
                    // Leave the second turn live so the test can exercise pause and crash recovery.
                    fs::File::create_new(home.join("second-step-submitted")).unwrap();
                    fs::write(stage, "working").unwrap();
                    println!(
                        r#"{{"result":{{"agent":{}}}}}"#,
                        agent.replace("idle", "working")
                    );
                    return;
                }
                fs::File::create_new(home.join("first-step-submitted")).unwrap();
                invoke
                    .args(report.split_whitespace().take(4))
                    .args(["continue", "Build fixed; verify on Windows"]);
            } else if prompt.contains("Current request:\nconfirm queue order\n")
                && !prompt.contains("queue order confirmed")
            {
                assert!(home.join("reply-posted").exists() && !stage.exists());
                invoke.args(["say", target, "queue order confirmed"]);
            } else if (prompt.contains("native-reply") && target.starts_with("room-")) || args[2]=="w1:p2" {
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
            let agent = if args[2] == "w1:p2" {
                agent.replace("tester", "peer").replace("w1:p1", "w1:p2")
            } else { agent };
            println!(r#"{{"result":{{"agent":{agent}}}}}"#)
        }
        (Some("agent"), Some("wait")) => {
            if args[3..7] == ["--until", "working", "--until", "blocked"] {
                assert_eq!(args[2], "w1:p1");
                fs::write(home.join("waiting-for-start"), "").unwrap();
                for _ in 0..400 {
                    if fs::read_to_string(&stage).unwrap() == "working" {
                        println!(
                            r#"{{"result":{{"agent":{}}}}}"#,
                            agent.replace("done", "working")
                        );
                        return;
                    }
                    if home.join("reply-posted").exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                println!(r#"{{"error":{{"code":"timeout","message":"wait timed out"}}}}"#);
                return;
            }
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
