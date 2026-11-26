use herdr_engine::{ipc, Engine};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn invalid_assignment(original: &Value, assignee: &str) -> Value {
    let mut changed = original.clone();
    changed["assigneeId"] = assignee.into();
    changed
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for task transition"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn a_room_task_stays_on_its_original_message_across_a_host_restart() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let fake = home.join(format!("fake-herdr{}", std::env::consts::EXE_SUFFIX));
    let cli = Path::new(env!("CARGO_BIN_EXE_herdr-bot-cli"));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr.rs");
    assert!(Command::new("rustc")
        .arg(fixture)
        .arg("-o")
        .arg(&fake)
        .status()
        .unwrap()
        .success());
    std::env::set_var("HERDR_BIN_PATH", fake);
    std::env::set_var("HERDR_BOT_SESSION", "native-test");
    let engine = Engine::start(home.into(), cli, |_| {}).unwrap();
    engine
        .call(
            "bot.create",
            json!({"id":"tester","name":"Tester","adoptPaneId":"w1:p1"}),
        )
        .unwrap();
    let room = engine
        .call(
            "room.create",
            json!({"name":"Tests","memberIds":["tester"]}),
        )
        .unwrap()["chat"]["id"]
        .clone();
    let message = engine
        .call(
            "message.send",
            json!({"chatId":room,"text":"Fix the build"}),
        )
        .unwrap();
    let task = json!({"chatId":room,"entryId":message["entryId"],"assigneeId":null,"criteria":"Windows build passes"});
    let mut invalid = task.clone();
    invalid["assigneeId"] = "outsider".into();
    assert!(engine.call("task.track", invalid).is_err());
    assert!(ipc::request(home, "task.track", task.clone()).is_err());
    let registered = engine.call("task.track", task.clone()).unwrap();
    assert_eq!(registered, engine.call("task.track", task.clone()).unwrap());
    assert_eq!(registered["id"], message["entryId"]);
    let mut assign = json!({"chatId":room,"entryId":message["entryId"],"previousAssigneeId":null,"assigneeId":"tester","version":null});
    assert!(engine.call("task.run", assign.clone()).is_err());
    assert!(ipc::request(home, "task.assign", assign.clone()).is_err());
    assert!(engine
        .call("task.assign", invalid_assignment(&assign, "outsider"))
        .is_err());
    engine.call("task.assign", assign.clone()).unwrap();
    assert!(engine
        .call("task.assign", invalid_assignment(&assign, "peer"))
        .is_err());
    assign["previousAssigneeId"] = "tester".into();
    assign["assigneeId"] = Value::Null;
    assert!(engine.call("task.assign", assign.clone()).unwrap()["task"]["assigneeId"].is_null());
    std::fs::write(home.join("two-bots"), "").unwrap();
    engine
        .call(
            "bot.create",
            json!({"id":"peer","name":"Peer","adoptPaneId":"w1:p2"}),
        )
        .unwrap();
    engine
        .call(
            "room.setMembers",
            json!({"roomId":room,"memberIds":["tester","peer"]}),
        )
        .unwrap();
    assign["previousAssigneeId"] = Value::Null;
    assign["assigneeId"] = "peer".into();
    engine.call("task.assign", assign.clone()).unwrap();
    assert!(engine
        .call("task.assign", invalid_assignment(&assign, "tester"))
        .is_err());
    assign["previousAssigneeId"] = "peer".into();
    assign["assigneeId"] = "tester".into();
    engine.call("task.assign", assign.clone()).unwrap();
    let mut edit = json!({"chatId":room,"entryId":message["entryId"],"previousStatus":"open","status":"in_progress"});
    let changed = engine.call("task.setStatus", edit.clone()).unwrap();
    edit["status"] = "completed".into();
    assert!(engine.call("task.setStatus", edit).is_err());
    let deadline = Instant::now() + Duration::from_secs(15);
    while engine.call("host.status", Value::Null).unwrap()["runningChatIds"] != json!([]) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    drop(engine);
    let restarted = loop {
        match Engine::start(home.into(), cli, |_| {}) {
            Ok(host) => break host,
            Err(error)
                if error.to_string().contains("another herdr-bot host")
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(error) => panic!("{error}"),
        }
    };
    let output = Command::new(cli)
        .env("HERDR_BOT_HOME", home)
        .args(["tasks", room.as_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!([changed])
    );
    let transcript = restarted
        .call("transcript.page", json!({"chatId":room}))
        .unwrap();
    assert!(transcript["entries"].as_array().unwrap().contains(&changed));

    let run =
        json!({"chatId":room,"entryId":message["entryId"],"assigneeId":"tester","version":null});
    assert!(ipc::request(home, "task.run", run.clone()).is_err());
    restarted.call("task.run", run.clone()).unwrap();
    assert!(restarted.call("task.run", run.clone()).is_err());
    wait_for(|| home.join("waiting-for-reply").exists());
    let current = restarted.call("task.list", json!({"chatId":room})).unwrap()[0].clone();
    let execution = &current["task"]["execution"];
    assert_eq!(execution["state"], "running");
    assert_eq!(execution["checkpoint"], "Build fixed; verify on Windows");

    // Snapshot the committed database with the second prompt already delivered.
    let recovery_dir = tempfile::tempdir().unwrap();
    let recovery = recovery_dir.path();
    rusqlite::Connection::open(home.join("chat.sqlite3"))
        .unwrap()
        .execute(
            "VACUUM INTO ?",
            [recovery.join("chat.sqlite3").to_str().unwrap()],
        )
        .unwrap();
    std::fs::write(recovery.join("delayed-stage"), "working").unwrap();
    let recovered = Engine::start(recovery.into(), cli, |_| {}).unwrap();
    wait_for(|| recovery.join("waiting-for-reply").exists());
    assert!(!recovery.join("second-step-submitted").exists());
    recovered
        .call(
            "message.send",
            json!({"chatId":room,"text":"confirm queue order"}),
        )
        .unwrap();

    let mut report = json!({"chatId":room,"entryId":message["entryId"],"attemptId":execution["attemptId"],"outcome":"completed","summary":"Windows build passed","paneId":"w1:p2"});
    assert!(ipc::request(recovery, "cli.taskReport", report.clone()).is_err());
    report["paneId"] = "w1:p1".into();
    let done = ipc::request(recovery, "cli.taskReport", report.clone()).unwrap();
    assert_eq!(done["task"]["status"], "completed");
    assert_eq!(
        ipc::request(recovery, "cli.taskReport", report.clone()).unwrap(),
        done
    );
    std::fs::write(recovery.join("reply-posted"), "").unwrap();
    wait_for(|| {
        let page = recovered
            .call("transcript.page", json!({"chatId":room}))
            .unwrap();
        page["entries"].as_array().unwrap().iter().any(|entry| {
            entry["author"]["id"] == "tester"
                && entry["message"]["content"] == "queue order confirmed"
        })
    });
    let replies = recovered
        .call(
            "transcript.thread",
            json!({"chatId":room,"rootId":message["entryId"]}),
        )
        .unwrap();
    assert_eq!(replies["entries"].as_array().unwrap().len(), 3);

    let pause = json!({"chatId":room,"entryId":message["entryId"],"version":execution["version"]});
    let pausing = restarted.call("task.pause", pause).unwrap();
    assign["previousAssigneeId"] = "tester".into();
    assign["assigneeId"] = "peer".into();
    assign["version"] = pausing["task"]["execution"]["version"].clone();
    assert!(restarted.call("task.assign", assign.clone()).is_err());
    report["outcome"] = "continue".into();
    report["summary"] = "Ready for review".into();
    let paused = ipc::request(home, "cli.taskReport", report.clone()).unwrap();
    assert_eq!(paused["task"]["execution"]["state"], "paused");
    std::fs::write(home.join("reply-posted"), "").unwrap();
    assign["version"] = paused["task"]["execution"]["version"].clone();
    let reassigned = restarted.call("task.assign", assign).unwrap();
    assert_eq!(reassigned["task"]["assigneeId"], "peer");
    assert_eq!(
        reassigned["task"]["execution"]["checkpoint"],
        "Ready for review"
    );
    assert!(ipc::request(home, "cli.taskReport", report).is_err());
    restarted
        .call(
            "room.setMembers",
            json!({"roomId":room,"memberIds":["tester"]}),
        )
        .unwrap();
    let released = restarted.call("task.list", json!({"chatId":room})).unwrap();
    assert!(released[0]["task"]["assigneeId"].is_null());
    assert_eq!(released[0]["task"]["execution"]["state"], "paused");
    for host in [&restarted, &recovered] {
        wait_for(|| host.call("host.status", Value::Null).unwrap()["runningChatIds"] == json!([]));
    }
}
