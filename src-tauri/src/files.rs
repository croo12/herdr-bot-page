use base64::{engine::general_purpose::STANDARD, Engine as _};
use herdr_engine::Engine;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{
    ipc::{InvokeBody, Request},
    State,
};
const MAX: u64 = 25 * 1024 * 1024;

fn file_name(raw: &str) -> String {
    let name = raw.rsplit(['/', '\\']).next().unwrap_or("attachment");
    let clean: String = name
        .chars()
        .filter(|c| !c.is_control() && !r#"<>:"/\|?*"#.contains(*c))
        .take(120)
        .collect();
    let clean = clean.trim_matches(['.', ' ']);
    if clean.is_empty() {
        "attachment".into()
    } else {
        clean.into()
    }
}
fn source(home: &Path, path: &str) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    let dir = fs::canonicalize(home.join("attachments")).ok()?;
    (path.is_file() && path.starts_with(dir)).then_some(path)
}
fn data_url(path: &Path, limit: u64) -> Option<String> {
    let f = fs::File::open(path).ok()?;
    if f.metadata().ok()?.len() > limit {
        return None;
    }
    let mut data = Vec::new();
    f.take(limit + 1).read_to_end(&mut data).ok()?;
    if data.len() as u64 > limit {
        return None;
    }
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(format!("data:{mime};base64,{}", STANDARD.encode(data)))
}
#[tauri::command]
pub fn stage(state: State<'_, Arc<Engine>>, request: Request<'_>) -> Value {
    let InvokeBody::Raw(bytes) = request.body() else {
        return json!({"ok":false,"reason":"empty"});
    };
    if bytes.is_empty() {
        return json!({"ok":false,"reason":"empty"});
    }
    if bytes.len() as u64 > MAX {
        return json!({"ok":false,"reason":"too-large"});
    }
    let name = request
        .headers()
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or("attachment".into());
    let dir = state.home.join("attachments");
    let path = dir.join(format!("{}-{}", uuid::Uuid::new_v4(), file_name(&name)));
    match fs::create_dir_all(&dir).and_then(|_| fs::write(&path, bytes)) {
        Ok(()) => json!({"ok":true,"path":path}),
        Err(_) => json!({"ok":false,"reason":"failed"}),
    }
}
#[tauri::command]
pub async fn attachment(
    state: State<'_, Arc<Engine>>,
    action: String,
    path: String,
    limit: Option<u64>,
    name: Option<String>,
) -> Result<Value, String> {
    let Some(path) = source(&state.home, &path) else {
        return Ok(Value::Null);
    };
    tauri::async_runtime::spawn_blocking(move||->std::io::Result<Value>{
        Ok(match action.as_str(){
            "discard"=>{fs::remove_file(path)?;Value::Null}
            "media"=>{let mime=mime_guess::from_path(&path).first_or_octet_stream();let kind=mime.type_().as_str();
                if !matches!(kind,"image"|"audio"|"video"){Value::Null}else{data_url(&path,MAX).map_or(Value::Null,|url|json!({"kind":kind,"dataUrl":url}))}}
            "text"=>{let f=fs::File::open(path)?;let size=f.metadata()?.len();let mut buf=Vec::new();f.take(1024*1024).read_to_end(&mut buf)?;
                if buf.iter().take(8000).any(|b|*b==0){json!({"kind":"binary","bytes":size})}else{json!({"kind":"text","text":String::from_utf8_lossy(&buf),"bytes":size,"truncated":size>buf.len() as u64})}}
            "bytes"=>{let size=fs::metadata(&path)?.len();if size>limit.unwrap_or(MAX).min(MAX){json!({"kind":"too-large","size":size})}else{json!({"kind":"bytes","base64":STANDARD.encode(fs::read(path)?)})}}
            "download"=>{let name=name.as_deref().or_else(||path.file_name()?.to_str()).unwrap_or("attachment");
                if let Some(dest)=rfd::FileDialog::new().set_file_name(file_name(name)).save_file(){fs::copy(path,dest)?;json!(true)}else{json!(false)}}
            _=>Value::Null
        })
    }).await.map_err(|e|e.to_string())?.map_err(|e|e.to_string())
}
#[tauri::command]
pub async fn pick(avatar: bool, default_path: Option<String>) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move||{
        let mut dialog=rfd::FileDialog::new();if let Some(dir)=default_path.filter(|p|Path::new(p).is_dir()){dialog=dialog.set_directory(dir);}
        if !avatar{return json!(dialog.pick_folder())}
        dialog.add_filter("Image",&["png","jpg","jpeg","gif","webp"]).pick_file().and_then(|p|Some(json!({"fileName":p.file_name()?.to_string_lossy(),"dataUrl":data_url(&p,MAX)?}))).unwrap_or(Value::Null)
    }).await.map_err(|e|e.to_string())
}
