use super::wire_error;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::Path,
    sync::Arc,
    thread,
    time::Duration,
};

const ENDPOINT: &str = "native-host.json";
const REQUEST_LIMIT: u64 = 4 * 1024 * 1024;
pub(crate) fn listen<Host: Send + Sync + 'static>(
    home: &Path,
    host: &Arc<Host>,
    handle: fn(&Arc<Host>, &str, Value) -> Result<Value>,
) -> Result<()> {
    let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let secret = uuid::Uuid::new_v4().to_string();
    let mut file = tempfile::NamedTempFile::new_in(home)?;
    serde_json::to_writer(
        &mut file,
        &json!({"port":socket.local_addr()?.port(),"token":secret}),
    )?;
    file.persist(home.join(ENDPOINT))?;
    let weak = Arc::downgrade(host);
    thread::spawn(move || {
        for client in socket.incoming().flatten() {
            let Some(host) = weak.upgrade() else { break };
            let secret = secret.clone();
            thread::spawn(move || {
                let _ = serve_client(client, &secret, |method, params| {
                    handle(&host, method, params)
                });
            });
        }
    });
    Ok(())
}
fn serve_client(
    mut stream: TcpStream,
    secret: &str,
    handle: impl FnOnce(&str, Value) -> Result<Value>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut line = String::new();
    BufReader::new(&stream)
        .take(REQUEST_LIMIT + 1)
        .read_line(&mut line)?;
    if line.len() as u64 > REQUEST_LIMIT {
        bail!("request too large")
    }
    let request: Value = serde_json::from_str(&line)?;
    if request["token"] != secret {
        bail!("invalid token")
    }
    let method = request["method"].as_str().unwrap_or_default();
    let res = handle(method, request["params"].clone());
    let response = match res {
        Ok(value) => json!({"value":value}),
        Err(error) => json!({"error":wire_error(&error)}),
    };
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    Ok(())
}
pub fn request(home: &Path, method: &str, params: Value) -> Result<Value> {
    let endpoint: Value = serde_json::from_slice(
        &fs::read(home.join(ENDPOINT)).context("Open herdr-bot or start herdr-bot serve first")?,
    )?;
    let port = endpoint["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .context("invalid host endpoint")?;
    let mut stream =
        TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, port).into(), Duration::from_secs(3))
            .context("host is offline")?;
    stream.set_read_timeout(Some(Duration::from_secs(150)))?;
    serde_json::to_writer(
        &mut stream,
        &json!({"token":endpoint["token"],"method":method,"params":params}),
    )?;
    stream.write_all(b"\n")?;
    let result: Value = serde_json::from_reader(stream)?;
    if let Some(error) = result.get("error") {
        bail!("{}", error["message"].as_str().unwrap_or("host error"))
    }
    Ok(result["value"].clone())
}
