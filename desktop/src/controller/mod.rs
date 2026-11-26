use crate::attachments::{data_url, MAX};
use herdr_engine::{wire_error, Engine};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
use tauri::State;
#[tauri::command]
pub(crate) async fn call(
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
pub(crate) fn open_external(url: String) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Unsupported link".into());
    }
    open::that_detached(parsed.as_str()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn pick(avatar: bool, default_path: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = default_path.filter(|p| Path::new(p).is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if !avatar {
            return json!(dialog.pick_folder());
        }
        dialog
            .add_filter("Image", &["png", "jpg", "jpeg", "gif", "webp"])
            .pick_file()
            .and_then(|path| {
                Some(json!({
                    "fileName": path.file_name()?.to_string_lossy(),
                    "dataUrl": data_url(&path, MAX)?
                }))
            })
            .unwrap_or(Value::Null)
    })
    .await
    .map_err(|e| e.to_string())
}
