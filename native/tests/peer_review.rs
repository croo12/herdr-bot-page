use herdr_engine::Engine;
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn wait(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn only_assigned_peer_can_decide_the_current_request() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let fake = home.join(format!("fake-herdr{}", std::env::consts::EXE_SUFFIX));
    assert!(Command::new("rustc")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/review_herdr.rs"))
        .arg("-o")
        .arg(&fake)
        .status()
        .unwrap()
        .success());
    std::env::set_var("HERDR_BIN_PATH", fake);
    std::env::set_var("HERDR_BOT_SESSION", "review-test");
    let cli = Path::new(env!("CARGO_BIN_EXE_herdr-bot-cli"));
    let engine = Engine::start(home.into(), cli, |_| {}).unwrap();
    for (id, pane) in [("worker", "p1"), ("peer", "p2")] {
        engine
            .call("bot.create", json!({"id":id,"name":id,"adoptPaneId":pane}))
            .unwrap();
    }
    let room = engine
        .call(
            "room.create",
            json!({"name":"Project","memberIds":["worker","peer"]}),
        )
        .unwrap()["chat"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    fs::write(home.join("room"), &room).unwrap();
    for decision in ["approve", "deny", "human"] {
        let reason = if decision == "deny" {
            "Use the test worktree"
        } else {
            "Test scope checked"
        };
        engine
            .call(
                "message.send",
                json!({"chatId":room,"clientNonce":decision,"text":"Run the library tests"}),
            )
            .unwrap();
        let ticket_path = home.join("ticket");
        wait(|| ticket_path.exists());
        let ticket = fs::read_to_string(&ticket_path).unwrap();
        fs::remove_file(ticket_path).unwrap();
        let decide = |pane: &str| {
            Command::new(cli)
                .env("HERDR_BOT_HOME", home)
                .env("HERDR_PANE_ID", pane)
                .args([
                    "say",
                    "--reply-to",
                    &ticket,
                    "--decision",
                    decision,
                    &room,
                    reason,
                ])
                .output()
                .unwrap()
        };
        assert!(!decide("p1").status.success());
        if decision == "approve" {
            fs::write(home.join("blocked"), "--all-targets").unwrap();
            assert!(!decide("p2").status.success());
            assert!(!home.join("key").exists());
            fs::write(home.join("blocked"), "--lib").unwrap();
        }
        if decision == "deny" {
            fs::write(home.join("reviewer-finishing"), "").unwrap();
        }
        let result = decide("p2");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!decide("p2").status.success());
        let transcript = engine
            .call("transcript.page", json!({"chatId":room}))
            .unwrap();
        let card = transcript["entries"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|entry| entry["kind"] == "prompt")
            .unwrap();
        let replies: Vec<_> = transcript["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["replyTo"] == card["id"])
            .collect();
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["message"]["content"], reason);
        assert_eq!(replies[0]["author"]["id"], "peer");
        assert_eq!(replies[0]["permissionDecision"], decision);
        assert_eq!(card["permissionReview"]["decision"], decision);
        assert_eq!(card["permissionReview"]["reviewerId"], "peer");
        assert_eq!(card["permissionReview"]["reason"], reason);
        if decision == "human" {
            assert_eq!(card["status"], "pending");
            assert!(!home.join("key").exists());
            engine.call("prompt.answer",json!({"botId":"worker","signature":card["prompt"]["signature"],"action":"cancel"})).unwrap();
        } else {
            assert_eq!(card["status"], "answered");
            assert_eq!(
                fs::read_to_string(home.join("key")).unwrap(),
                if decision == "approve" {
                    "enter"
                } else {
                    "esc"
                }
            );
        }
        if decision == "deny" || decision == "human" {
            wait(|| engine.call("host.status", json!({})).unwrap()["runningChatIds"] == json!([]));
            let page = engine
                .call("transcript.page", json!({"chatId":room}))
                .unwrap();
            let entries = page["entries"].as_array().unwrap();
            let mut after_denial = entries
                .iter()
                .filter(|entry| entry["seq"].as_i64().unwrap() > card["seq"].as_i64().unwrap());
            assert_eq!(
                after_denial
                    .clone()
                    .filter(|entry| entry["author"]["id"] == "worker"
                        && entry["message"]["content"]
                            == "I will request permission for the corrected worktree.")
                    .count(),
                1
            );
            if decision == "deny" {
                assert!(!after_denial.any(|entry| entry["kind"] == "notice"));
                assert!(home.join("denial-follow-up").exists());
                fs::remove_file(home.join("reviewer-finishing")).unwrap();
            }
        }
        fs::remove_file(home.join("key")).unwrap();
        // Let the mirror observe the end of this request before repeating it.
        wait(|| {
            let bots = engine.call("chat.list", json!({})).unwrap();
            bots.as_array()
                .unwrap()
                .iter()
                .any(|bot| bot["id"] == "worker" && bot["bot"]["status"] == "idle")
        });
    }
    verify_delegation_thread(&engine, home);
}

fn verify_delegation_thread(engine: &std::sync::Arc<Engine>, home: &Path) {
    let room = engine
        .call(
            "room.create",
            json!({"name":"Delegation","memberIds":["peer"]}),
        )
        .unwrap()["chat"]["id"]
        .clone();
    fs::write(home.join("delegation-room"), room.as_str().unwrap()).unwrap();
    let message = engine
        .call(
            "message.send",
            json!({"chatId":room,"text":"Delegate build verification"}),
        )
        .unwrap();
    let thread = || {
        engine
            .call(
                "transcript.thread",
                json!({"chatId":room,"rootId":message["entryId"]}),
            )
            .unwrap()["entries"]
            .clone()
    };
    wait(|| thread()[1]["delegation"]["state"] == "waiting");
    let tracker = thread()[1].clone();
    let prompt_id = &tracker["delegation"]["promptId"];
    let target = &tracker["delegation"]["targetChatId"];
    let prompt_chat = &tracker["delegation"]["promptChatId"];
    assert_ne!(target, prompt_chat);
    assert!(engine
        .call("transcript.page", json!({"chatId":target}))
        .is_err());
    let inbox_chat = engine
        .call("transcript.page", json!({"chatId":target,"paneId":"p1"}))
        .unwrap();
    assert!(inbox_chat["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["delegatedFrom"]["entryId"] == tracker["id"]));
    let worker = engine
        .call("transcript.page", json!({"chatId":prompt_chat}))
        .unwrap();
    let entries = worker["entries"].as_array().unwrap();
    let prompt = entries
        .iter()
        .find(|entry| entry["id"] == *prompt_id)
        .unwrap();
    assert_eq!(prompt["kind"], "prompt");
    engine.call("prompt.answer",json!({"botId":"worker","signature":prompt["prompt"]["signature"],"action":"option","key":prompt["prompt"]["options"][0]["key"]})).unwrap();
    wait(|| thread()[1]["delegation"]["state"] == "completed");
    let finished = thread();
    assert_eq!(finished.as_array().unwrap().len(), 3, "{finished}");
    assert!(finished[1]["delegation"]["promptId"].is_null());
    assert_eq!(finished[2]["author"]["id"], "worker");
    assert_eq!(finished[2]["delegationId"], tracker["id"]);
    assert_eq!(
        finished[2]["message"]["content"],
        "Verified the delegated build"
    );
    wait(|| engine.call("host.status", json!({})).unwrap()["runningChatIds"] == json!([]));
    fs::write(home.join("delegation-failure"), "").unwrap();
    let retry = engine
        .call("message.send", json!({"chatId":room,"text":"Check again"}))
        .unwrap();
    let retry_thread = || {
        engine
            .call(
                "transcript.thread",
                json!({"chatId":room,"rootId":retry["entryId"]}),
            )
            .unwrap()["entries"]
            .clone()
    };
    wait(|| retry_thread()[1]["delegation"]["state"] == "failed");
    assert!(retry_thread()[1]["delegation"]["detail"]
        .as_str()
        .unwrap()
        .contains("agent_unavailable"));
    let inbox = engine.call("cli.inbox", json!({"paneId":"p1"})).unwrap();
    let delivery = inbox
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["chatId"] == *target && item["state"] == "unconfirmed")
        .unwrap();
    engine
        .call(
            "cli.inbox",
            json!({"paneId":"p1","ack":delivery["messageId"]}),
        )
        .unwrap();
    assert_eq!(retry_thread()[1]["delegation"]["state"], "completed");
    assert_eq!(thread(), finished);
}
