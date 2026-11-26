use super::service;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use herdr_engine::Engine;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{
    ipc::{InvokeBody, Request},
    State,
};
#[tauri::command]
pub fn stage(state: State<'_, Arc<Engine>>, request: Request<'_>) -> Value {
    let InvokeBody::Raw(bytes) = request.body() else {
        return json!({"ok":false,"reason":"empty"});
    };
    let name = request
        .headers()
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or("attachment".into());
    service::stage(&state.home, &name, bytes)
}
#[tauri::command]
pub async fn attachment(
    state: State<'_, Arc<Engine>>,
    action: String,
    path: String,
    limit: Option<u64>,
    name: Option<String>,
) -> Result<Value, String> {
    let home = state.home.clone();
    tauri::async_runtime::spawn_blocking(move || {
        service::access(&home, &action, &path, limit, name.as_deref(), |name| {
            rfd::FileDialog::new().set_file_name(name).save_file()
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}
