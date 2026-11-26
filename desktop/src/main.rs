#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod attachments;
mod controller;
use herdr_engine::{default_home, Engine};
use tauri::{Emitter, Manager};

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
            controller::call,
            controller::open_external,
            attachments::controller::stage,
            attachments::controller::attachment,
            controller::pick
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        rfd::MessageDialog::new()
            .set_title("herdr-bot")
            .set_description(error.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
        std::process::exit(1);
    }
}
