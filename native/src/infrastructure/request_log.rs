use crate::domain::now;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const MAX_BYTES: u64 = 5 * 1024 * 1024;

pub(crate) struct RequestLog {
    path: PathBuf,
    writer: Mutex<()>,
}

impl RequestLog {
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            path: home.join("requests.jsonl"),
            writer: Mutex::new(()),
        }
    }

    pub(crate) fn record(
        &self,
        source: &str,
        method: &str,
        elapsed: Duration,
        error: Option<&str>,
    ) {
        let mut record = json!({
            "timestampMs": now(),
            "source": source,
            "method": method,
            "durationMs": elapsed.as_secs_f64() * 1000.0,
            "ok": error.is_none(),
        });
        if let Some(code) = error {
            record["errorCode"] = json!(code);
        }
        let line = format!("{record}\n");
        if let Err(error) = self.append(line.as_bytes()) {
            let _ = writeln!(
                io::stderr().lock(),
                "request log write failed ({error}): {record}"
            );
        }
    }

    fn append(&self, line: &[u8]) -> io::Result<()> {
        let _guard = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        match fs::metadata(&self.path) {
            Ok(info) if info.len().saturating_add(line.len() as u64) > MAX_BYTES => {
                fs::rename(&self.path, self.path.with_extension("1.jsonl"))?;
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_replaces_the_previous_backup() -> io::Result<()> {
        let home = tempfile::tempdir()?;
        let log = RequestLog::new(home.path());
        let backup = home.path().join("requests.1.jsonl");
        fs::write(&backup, b"older")?;
        fs::File::create(&log.path)?.set_len(MAX_BYTES)?;
        log.append(b"{}\n")?;
        assert_eq!(fs::metadata(backup)?.len(), MAX_BYTES);
        assert_eq!(fs::read(log.path)?, b"{}\n");
        Ok(())
    }
}
