use herdr_engine::ipc;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn cli(home: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_herdr-bot-cli"))
        .env("HERDR_BOT_HOME", home)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn start(home: &Path, fake: &Path) -> Host {
    let child = Command::new(env!("CARGO_BIN_EXE_herdr-bot-cli"))
        .env("HERDR_BOT_HOME", home)
        .env("HERDR_BIN_PATH", fake)
        .env("HERDR_BOT_SESSION", "native-test")
        .arg("serve")
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let host = Host(child);
    wait(|| ipc::request(home, "host.status", Value::Null).is_ok());
    host
}
fn wait(mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(15);
    while !ok() {
        assert!(Instant::now() < until, "host operation timed out");
        thread::sleep(Duration::from_millis(50));
    }
}
fn compile_herdr_fixture(directory: &Path) -> PathBuf {
    let fake = directory.join(format!("fake-herdr{}", std::env::consts::EXE_SUFFIX));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr.rs");
    assert!(Command::new("rustc")
        .arg(fixture)
        .arg("-o")
        .arg(&fake)
        .status()
        .unwrap()
        .success());
    fake
}

#[test]
fn quick_create_registers_an_existing_agent_and_greets_in_its_direct_chat() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let fake = compile_herdr_fixture(home);
    let _host = start(home, &fake);
    ipc::request(
        home,
        "settings.update",
        json!({"defaultProvider":"codex","defaultPermissionMode":"ask"}),
    )
    .unwrap();
    let defaults = ipc::request(home, "bot.defaults", Value::Null).unwrap();
    assert_eq!(defaults["provider"], "codex");
    assert_eq!(defaults["permissionMode"], "ask");
    for method in ["bot.create", "bot.quickCreate"] {
        for provider in [None, Some(Value::Null), Some(json!("")), Some(json!("  "))] {
            let mut request = json!({"name":"Missing provider", "cwd":home});
            if let Some(value) = provider {
                request["provider"] = value;
            }
            let error = ipc::request(home, method, request).unwrap_err();
            assert!(error.to_string().contains("missing provider"), "{error}");
        }
    }
    let params = json!({"id":"tester","adoptPaneId":"w1:p1","requestId":"quick","locale":"en",
        "title":"Reviewer","description":"Review changes","avatarColor":"violet"});
    let call = || ipc::request(home, "bot.quickCreate", params.clone()).unwrap();
    let profile = call()["chat"].clone();
    assert_eq!(profile["id"], "tester");
    assert_eq!(profile["name"], "pi");
    assert_eq!(profile["bot"]["provider"], "pi");
    assert_eq!(profile["bot"]["cwd"], ".");
    assert_eq!(profile["bot"]["adopted"], true);
    assert_eq!(profile["title"], "Reviewer");
    assert_eq!(profile["description"], "Review changes");
    assert_eq!(profile["avatarColor"], "violet");
    assert_eq!(call()["chat"]["id"], profile["id"]);
    let direct = ipc::request(home, "chat.openDirect", json!({"botId":"tester"})).unwrap();
    wait(|| {
        let page = ipc::request(home, "transcript.page", json!({"chatId":direct["id"]})).unwrap();
        page["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["author"]["id"] == "tester")
    });
}

#[test]
fn each_bot_discovers_its_own_direct_messages_after_a_host_restart() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let fake = compile_herdr_fixture(home);
    std::fs::write(home.join("two-bots"), "").unwrap();
    let host = start(home, &fake);
    for (bot, pane) in [("tester", "w1:p1"), ("peer", "w1:p2")] {
        cli(home, &["bot", "adopt", pane, bot]);
    }
    // Both bots are busy doing work outside this host. No turn can consume the deliveries.
    std::fs::write(home.join("delayed-stage"), "working").unwrap();
    let call = |method, params| ipc::request(home, method, params).unwrap();
    let dm = call("chat.openDirect", json!({"botId":"tester"}));
    let sent = call(
        "message.send",
        json!({"chatId":dm["id"],"text":"from the user","clientNonce":"one"}),
    );
    assert_eq!(
        call(
            "message.send",
            json!({"chatId":"tester","text":"from the user","clientNonce":"one"})
        ),
        sent
    );
    let pair = call(
        "cli.dm",
        json!({"paneId":"w1:p2","chatId":"tester","text":"from the peer"}),
    );
    assert_ne!(pair["chatId"], dm["id"]);
    assert_eq!(
        call("chat.openDirect", json!({"botId":"peer","paneId":"w1:p1"}))["id"],
        pair["chatId"]
    );
    assert!(ipc::request(home, "transcript.page", json!({"chatId":pair["chatId"]})).is_err());
    assert!(call("chat.list", Value::Null)
        .as_array()
        .unwrap()
        .iter()
        .all(|chat| chat["id"] != pair["chatId"]));
    call("chat.markRead", json!({"chatId":dm["id"],"throughSeq":1}));
    assert_eq!(call("cli.inbox", json!({"paneId":"w1:p2"})), json!([]));
    drop(host);
    let _host = start(home, &fake);
    let inbox = call("cli.inbox", json!({"paneId":"w1:p1"}));
    let rows = inbox.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["messageId"], sent["entryId"]);
    assert_eq!(rows[1]["chatId"], pair["chatId"]);
    assert_eq!(rows[1]["author"]["id"], "peer");
    assert_eq!(rows[1]["text"], "from the peer");
    assert!(ipc::request(home, "cli.inbox", json!({"paneId":"unregistered"})).is_err());
    std::fs::remove_file(home.join("delayed-stage")).unwrap();
    wait(|| {
        call("cli.inbox", json!({"paneId":"w1:p1"}))
            .as_array()
            .unwrap()
            .is_empty()
    });
    let history = call("transcript.page", json!({"chatId":dm["id"]}));
    assert_eq!(
        history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "send-message")
            .count(),
        1
    );
}

#[test]
fn bot_subscribers_run_independently_in_one_room_and_serialize_across_their_chats() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let fake = compile_herdr_fixture(home);
    std::fs::write(home.join("two-bots"), "").unwrap();
    std::fs::write(home.join("bot-workers"), "").unwrap();
    let _host = start(home, &fake);
    for (bot, pane) in [("tester", "w1:p1"), ("peer", "w1:p2")] {
        cli(home, &["bot", "adopt", pane, bot]);
    }
    let call = |method, params| ipc::request(home, method, params).unwrap();
    let room = call(
        "room.create",
        json!({"name":"Build","memberIds":["tester","peer"]}),
    )["chat"]["id"]
        .clone();
    let dm = call("chat.openDirect", json!({"botId":"tester"}))["id"].clone();
    let replies = |chat: &Value| {
        call("transcript.page", json!({"chatId":chat}))["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["kind"] == "send-message")
            .map(|entry| entry["message"]["content"].clone())
            .collect::<Vec<_>>()
    };
    call(
        "message.send",
        json!({"chatId":room,"text":"@tester build"}),
    );
    wait(|| home.join("build-started").exists());
    let follow_up = call("message.send", json!({"chatId":dm,"text":"Follow up"}));
    call("message.send", json!({"chatId":room,"text":"@peer review"}));
    wait(|| replies(&room) == vec![json!("Review complete")]);
    wait(|| call("cli.inbox", json!({"paneId":"w1:p2"})) == json!([]));
    let chats = call("chat.list", Value::Null);
    let activity = |id: &Value| {
        chats
            .as_array()
            .unwrap()
            .iter()
            .find(|chat| &chat["id"] == id)
            .unwrap()["isRunning"]
            .clone()
    };
    assert_eq!(activity(&room), true);
    assert_eq!(activity(&dm), false);
    let inbox = call("cli.inbox", json!({"paneId":"w1:p1"}));
    assert!(inbox
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["messageId"] == follow_up["entryId"] && row["state"] == "pending"));
    assert!(replies(&dm).is_empty());
    std::fs::write(home.join("release-build"), "").unwrap();
    wait(|| replies(&dm) == vec![json!("Follow-up complete")]);
    wait(|| call("host.status", Value::Null)["runningChatIds"] == json!([]));
    assert_eq!(
        replies(&room),
        vec![json!("Review complete"), json!("Build complete")]
    );
}

#[test]
fn room_reply_wakes_the_requesting_bot_and_stays_in_the_original_thread() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let fake = compile_herdr_fixture(home);
    std::fs::write(home.join("two-bots"), "").unwrap();
    std::fs::write(home.join("room-replies"), "").unwrap();
    let _host = start(home, &fake);
    for (bot, pane) in [("tester", "w1:p1"), ("peer", "w1:p2")] {
        cli(home, &["bot", "adopt", pane, bot]);
    }
    let call = |method, params| ipc::request(home, method, params).unwrap();
    let room = call(
        "room.create",
        json!({"name":"Packaging","memberIds":["tester","peer"]}),
    )["chat"]["id"]
        .clone();
    let root = call(
        "message.send",
        json!({"chatId":room,"text":"@tester check packaging"}),
    )["entryId"]
        .clone();
    let thread =
        || call("transcript.thread", json!({"chatId":room,"rootId":root}))["entries"].clone();
    wait(|| thread()[3]["message"]["content"] == "I will check signing");
    let entries = thread();
    let entries = entries.as_array().unwrap();
    assert_eq!(entries.len(), 4);
    for (pair, author) in entries.windows(2).zip(["tester", "peer", "tester"]) {
        assert_eq!(pair[1]["replyTo"], pair[0]["id"]);
        assert_eq!(pair[1]["author"]["id"], author);
    }
    let pending = call("cli.inbox", json!({"paneId":"w1:p2"}));
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(pending[0]["messageId"], entries[3]["id"]);
}

#[test]
fn stalled_prompt_keeps_the_original_turn_until_start_or_reply_is_confirmed() {
    let directory = tempfile::tempdir().unwrap();
    let fake = compile_herdr_fixture(directory.path());
    for scenario in ["late-start", "missed-start", "never-started"] {
        let home = directory.path().join(scenario);
        let _host = start(&home, &fake);
        cli(&home, &["bot", "adopt", "w1:p1", "tester"]);
        ipc::request(
            &home,
            "message.send",
            json!({"chatId":"tester","text":"simulate stalled turn"}),
        )
        .unwrap();
        let finished = || {
            ipc::request(&home, "host.status", Value::Null).unwrap()["runningChatIds"] == json!([])
        };
        let start_wait = home.join("waiting-for-start");
        wait(|| start_wait.exists() || finished());
        assert!(
            start_wait.exists(),
            "{scenario}: the host abandoned the stalled prompt"
        );
        match scenario {
            "late-start" => {
                std::fs::write(home.join("delayed-stage"), "working").unwrap();
                wait(|| home.join("waiting-for-reply").exists());
                let response = ipc::request(
                    &home,
                    "cli.pass",
                    json!({"paneId":"w1:p1","chatId":"tester"}),
                )
                .unwrap();
                assert_eq!(response["passed"], true);
            }
            "missed-start" => {
                ipc::request(
                    &home,
                    "cli.say",
                    json!({"paneId":"w1:p1","chatId":"tester","text":"late reply"}),
                )
                .unwrap();
            }
            _ => {}
        }
        std::fs::write(home.join("reply-posted"), "").unwrap();
        wait(finished);
        let page = ipc::request(&home, "transcript.page", json!({"chatId":"tester"})).unwrap();
        let entries = page["entries"].as_array().unwrap();
        match scenario {
            "late-start" => assert_eq!(entries.len(), 1, "{entries:?}"),
            "missed-start" => {
                assert_eq!(entries.len(), 2, "{entries:?}");
                assert_eq!(entries[1]["message"]["content"], "late reply");
            }
            _ => {
                assert_eq!(entries.len(), 2, "{entries:?}");
                assert!(entries[1]["content"]
                    .as_str()
                    .unwrap()
                    .contains("agent_prompt_stalled"));
            }
        }
        assert_eq!(std::fs::read(home.join("prompt-count")).unwrap(), b"1");
    }
}

#[test]
fn cli_and_desktop_contract_share_turns_events_and_durable_history() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("테스트 home");
    let fake = compile_herdr_fixture(tmp.path());
    let host = start(&home, &fake);
    cli(&home, &["bot", "adopt", "w1:p1", "tester"]);
    let room: Value = serde_json::from_str(&cli(
        &home,
        &["room", "create", "테스트 방", "--members", "tester"],
    ))
    .unwrap();
    let id = room["chat"]["id"].as_str().unwrap();
    let params = json!({"chatId":id,"clientNonce":"same-request","text":"hello"});
    let first = ipc::request(&home, "message.send", params.clone()).unwrap();
    assert_eq!(ipc::request(&home, "message.send", params).unwrap(), first);
    wait(|| cli(&home, &["read", id]).contains("native-reply 안녕하세요"));
    wait(|| {
        ipc::request(&home, "host.status", Value::Null).unwrap()["runningChatIds"] == json!([])
    });
    let entries =
        ipc::request(&home, "transcript.page", json!({"chatId":id})).unwrap()["entries"].clone();
    assert_eq!(entries.as_array().unwrap().len(), 2);
    let entry = &entries[1]["id"];
    let reaction = ipc::request(
        &home,
        "message.react",
        json!({"chatId":id,"entryId":entry,"emoji":"👍"}),
    )
    .unwrap();
    assert_eq!(reaction["entry"]["reactions"][0]["emoji"], "👍");
    let hits = ipc::request(&home, "message.search", json!({"query":"안녕"})).unwrap();
    assert_eq!(hits[0]["entryId"], *entry);
    let outsider = ipc::request(
        &home,
        "cli.say",
        json!({"paneId":"w1:p1","chatId":id,"text":"late"}),
    );
    assert!(outsider.is_err());
    let denied = ipc::request(
        &home,
        "prompt.answer",
        json!({"signature":"private-marker"}),
    );
    assert!(denied
        .unwrap_err()
        .to_string()
        .contains("requires the desktop app"));
    drop(host);
    let log = std::fs::read_to_string(home.join("requests.jsonl")).unwrap();
    assert!(!log.contains("private-marker"));
    assert!(!log.contains("same-request"));
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(records.iter().all(|row| row["source"] == "ipc"
        && row["timestampMs"].as_u64().unwrap() > 0
        && row["durationMs"].as_f64().unwrap() >= 0.0));
    assert!(records
        .iter()
        .any(|row| row["method"] == "message.send" && row["ok"] == true));
    assert!(records.iter().any(|row| row["method"] == "prompt.answer"
        && row["ok"] == false
        && row["errorCode"] == "invalid_args"));
    let _restart = start(&home, &fake);
    assert!(cli(&home, &["read", id]).contains("native-reply 안녕하세요"));
    assert_eq!(
        ipc::request(&home, "host.status", Value::Null).unwrap()["bots"],
        1
    );
    // A delivered prompt can outlive several Herdr status waits. Its bot can
    // still reply to the original chat, and the prompt is submitted only once.
    ipc::request(
        &home,
        "message.send",
        json!({"chatId":"tester","clientNonce":"delayed","text":"simulate delayed turn"}),
    )
    .unwrap();
    wait(|| home.join("waiting-for-reply").exists());
    let dm = ipc::request(&home, "chat.openDirect", json!({"botId":"tester"})).unwrap();
    assert_ne!(dm["id"], "tester");
    assert_eq!(
        ipc::request(&home, "host.status", Value::Null).unwrap()["runningChatIds"],
        json!([dm["id"]])
    );
    ipc::request(
        &home,
        "cli.say",
        json!({"paneId":"w1:p1","chatId":"tester","text":"finished after approval"}),
    )
    .unwrap();
    std::fs::write(home.join("reply-posted"), "").unwrap();
    wait(|| {
        ipc::request(&home, "host.status", Value::Null).unwrap()["runningChatIds"] == json!([])
    });
    let transcript = ipc::request(&home, "transcript.page", json!({"chatId":"tester"})).unwrap();
    let entries = transcript["entries"].as_array().unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry["message"]["content"] == "finished after approval"));
    assert!(!entries.iter().any(|entry| entry["kind"] == "notice"));
}
