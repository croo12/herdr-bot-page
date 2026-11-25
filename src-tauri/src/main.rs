#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod files;
use herdr_engine::{default_home, wire_error, Engine};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

#[tauri::command]
async fn call(
    engine: State<'_, Arc<Engine>>,
    method: String,
    params: Option<Value>,
) -> Result<Value, Value> {
    let engine = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        engine
            .call(&method, params.unwrap_or(Value::Null))
            .map_err(|e| wire_error(&e))
    })
    .await
    .map_err(|e| json!({"code":"internal","message":e.to_string()}))?
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Unsupported link".into());
    }
    open::that_detached(parsed.as_str()).map_err(|e| e.to_string())
}

fn main() {
    let result = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let exe = std::env::current_exe()?;
            let cli = exe.with_file_name(if cfg!(windows) {
                "herdr-bot-cli.exe"
            } else {
                "herdr-bot-cli"
            });
            let engine = Engine::start(default_home(), &cli, move |event| {
                let _ = handle.emit("herdr-event", event);
            })?;
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            call,
            open_external,
            files::stage,
            files::attachment,
            files::pick
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        rfd::MessageDialog::new()
            .set_title("herdr-bot")
            .set_description(e.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}
