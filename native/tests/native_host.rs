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
#[test]
fn cli_and_desktop_contract_share_turns_events_and_durable_history() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("테스트 home");
    let fake = tmp
        .path()
        .join(format!("fake-herdr{}", std::env::consts::EXE_SUFFIX));
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/herdr.rs");
    assert!(Command::new("rustc")
        .arg(fixture)
        .arg("-o")
        .arg(&fake)
        .status()
        .unwrap()
        .success());
    let host = start(&home, &fake);
    cli(&home, &["bot", "adopt", "w1:p1", "tester"]);
    let room: Value = serde_json::from_str(&cli(
        &home,
        &["room", "create", "테스트 방", "--members", "tester"],
    ))
    .unwrap();
    let id = room["chat"]["id"].as_str().unwrap();
    let p = json!({"chatId":id,"clientNonce":"same-request","text":"hello"});
    let first = ipc::request(&home, "message.send", p.clone()).unwrap();
    assert_eq!(ipc::request(&home, "message.send", p).unwrap(), first);
    wait(|| cli(&home, &["read", id]).contains("native-reply 안녕하세요"));
    wait(|| {
        ipc::request(&home, "host.status", Value::Null).unwrap()["runningChatIds"] == json!([])
    });
    let entries =
        ipc::request(&home, "transcript.page", json!({"chatId":id})).unwrap()["entries"].clone();
    assert_eq!(entries.as_array().unwrap().len(), 2);
    let entry = &entries[1]["id"];
    let r = ipc::request(
        &home,
        "message.react",
        json!({"chatId":id,"entryId":entry,"emoji":"👍"}),
    )
    .unwrap();
    assert_eq!(r["entry"]["reactions"][0]["emoji"], "👍");
    let hits = ipc::request(&home, "message.search", json!({"query":"안녕"})).unwrap();
    assert_eq!(hits[0]["entryId"], *entry);
    let outsider = ipc::request(
        &home,
        "cli.say",
        json!({"paneId":"w1:p1","chatId":id,"text":"late"}),
    );
    assert!(outsider.is_err());
    drop(host);
    let _restart = start(&home, &fake);
    assert!(cli(&home, &["read", id]).contains("native-reply 안녕하세요"));
    assert_eq!(
        ipc::request(&home, "host.status", Value::Null).unwrap()["bots"],
        1
    );
}
