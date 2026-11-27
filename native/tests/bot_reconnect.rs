use herdr_engine::Engine;
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn a_room_recovers_missing_managed_and_relocated_agents_before_sending() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path();
    let fake = home.join(format!("fake-herdr{}", std::env::consts::EXE_SUFFIX));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reconnect_herdr.rs");
    assert!(Command::new("rustc")
        .arg(fixture)
        .arg("-o")
        .arg(&fake)
        .status()
        .unwrap()
        .success());
    std::env::set_var("HERDR_BIN_PATH", fake);
    std::env::set_var("HERDR_BOT_SESSION", "reconnect-test");
    let engine = Engine::start(
        home.into(),
        Path::new(env!("CARGO_BIN_EXE_herdr-bot-cli")),
        |_| {},
    )
    .unwrap();
    for request in [
        json!({"id":"external","name":"Tester","provider":"pi","cwd":home}),
        json!({"id":"external","name":"Tester","adoptPaneId":"w5:p2"}),
    ] {
        let error = engine.call("bot.create", request).unwrap_err();
        assert_eq!(
            error.to_string(),
            "bot_exists: agent external already exists"
        );
    }
    let managed = engine
        .call(
            "bot.create",
            json!({"id":"managed","name":"Managed","provider":"codex","cwd":home,
                "model":"test-model","reasoningEffort":"high","permissionMode":"auto",
                "title":"UI reviewer","description":"Check the UI for accessibility"}),
        )
        .unwrap();
    assert_eq!(managed["chat"]["bot"]["adopted"], false);
    let available = engine.call("bot.listAdoptable", Value::Null).unwrap();
    assert_eq!(available.as_array().unwrap().len(), 2);
    assert!(available
        .as_array()
        .unwrap()
        .iter()
        .any(|agent| agent["paneId"] == "w5:p3" && agent["provider"] == "claude"));
    for (bot, pane) in [("moved", "w5:p2"), ("external", "w5:p3")] {
        let adopted = engine
            .call(
                "bot.create",
                json!({"id":bot,"name":bot,"adoptPaneId":pane}),
            )
            .unwrap();
        assert_eq!(adopted["chat"]["bot"]["paneId"], pane);
        assert_eq!(adopted["chat"]["bot"]["adopted"], true);
    }
    let directory = home.join("relaunch");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(home.join("fail-replacement"), "").unwrap();
    assert!(engine
        .call(
            "bot.updateLaunchSettings",
            json!({"botId":"external","cwd":directory})
        )
        .is_err());
    assert!(home.join("name-restored").exists());
    assert!(home.join("replacement-closed").exists());
    std::fs::remove_file(home.join("fail-replacement")).unwrap();
    let after_failure = engine.call("chat.list", Value::Null).unwrap();
    let external = after_failure
        .as_array()
        .unwrap()
        .iter()
        .find(|profile| profile["id"] == "external")
        .unwrap();
    assert_eq!(external["bot"]["paneId"], "w5:p3");
    assert_eq!(external["bot"]["adopted"], true);
    let room = engine
        .call(
            "room.create",
            json!({"name":"Recovery","memberIds":["managed","moved"]}),
        )
        .unwrap()["chat"]["id"]
        .clone();
    std::fs::write(home.join("reset"), "").unwrap();
    std::fs::write(home.join("external-closed"), "").unwrap();
    assert!(engine
        .call("bot.connect", json!({"botId":"external"}))
        .unwrap_err()
        .to_string()
        .contains("adopted agent"));
    engine
        .call(
            "message.send",
            json!({"chatId":room,"text":"review the UI"}),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while engine.call("host.status", Value::Null).unwrap()["runningChatIds"] != json!([]) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(30));
    }
    let transcript = engine
        .call("transcript.page", json!({"chatId":room}))
        .unwrap();
    let entries = transcript["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3, "{entries:?}");
    for (bot, pane) in [("managed", "w8:p1"), ("moved", "w8:p2")] {
        assert!(entries
            .iter()
            .any(|entry| entry["author"]["id"] == bot && entry["message"]["content"] == bot));
        let chats = engine.call("chat.list", Value::Null).unwrap();
        assert!(chats
            .as_array()
            .unwrap()
            .iter()
            .any(|chat| chat["id"] == bot && chat["bot"]["paneId"] == pane));
        assert_eq!(
            engine.call("bot.connect", json!({"botId":bot})).unwrap()["pane_id"],
            pane
        );
    }
    // The adopted Claude agent never reported a model. Relaunch must keep its CLI default.
    let relaunched = engine
        .call(
            "bot.updateLaunchSettings",
            json!({"botId":"external","cwd":directory}),
        )
        .unwrap();
    assert_eq!(relaunched["chat"]["bot"]["paneId"], "w9:p1");
    assert_eq!(relaunched["chat"]["bot"]["model"], Value::Null);
    let cli_options = || std::fs::read_to_string(home.join("last-launch-options")).unwrap();
    assert!(cli_options().is_empty());
    engine
        .call(
            "bot.updateLaunchSettings",
            json!({"botId":"external","model":"custom","reasoningEffort":"high"}),
        )
        .unwrap();
    let retried = engine
        .call("bot.retrySetup", json!({"botId":"external"}))
        .unwrap();
    assert_eq!(retried["chat"]["bot"]["model"], "custom");
    assert!(cli_options().contains("--model custom"));
    let cleared = engine
        .call(
            "bot.updateLaunchSettings",
            json!({"botId":"external","model":null}),
        )
        .unwrap();
    assert_eq!(cleared["chat"]["bot"]["model"], Value::Null);
    assert_eq!(cleared["chat"]["bot"]["reasoningEffort"], "high");
    assert!(!cli_options().contains("--model"));
    assert!(cli_options().contains("--effort high"));
}
