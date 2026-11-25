use crate::{wire_error, Engine};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value as V};
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
pub(crate) fn listen(engine: &Arc<Engine>) -> Result<()> {
    let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let secret = uuid::Uuid::new_v4().to_string();
    let mut file = tempfile::NamedTempFile::new_in(&engine.home)?;
    serde_json::to_writer(
        &mut file,
        &json!({"port":socket.local_addr()?.port(),"token":secret}),
    )?;
    file.persist(engine.home.join(ENDPOINT))?;
    let weak = Arc::downgrade(engine);
    thread::spawn(move || {
        for client in socket.incoming().flatten() {
            let Some(engine) = weak.upgrade() else { break };
            let secret = secret.clone();
            thread::spawn(move || {
                let _ = serve_client(client, &engine, &secret);
            });
        }
    });
    Ok(())
}
fn serve_client(mut stream: TcpStream, engine: &Arc<Engine>, secret: &str) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut line = String::new();
    BufReader::new(&stream)
        .take(REQUEST_LIMIT + 1)
        .read_line(&mut line)?;
    if line.len() as u64 > REQUEST_LIMIT {
        bail!("request too large")
    }
    let v: V = serde_json::from_str(&line)?;
    if v["token"] != secret {
        bail!("invalid token")
    }
    let res = engine.call(
        v["method"].as_str().unwrap_or_default(),
        v["params"].clone(),
    );
    let response = match res {
        Ok(value) => json!({"value":value}),
        Err(e) => json!({"error":wire_error(&e)}),
    };
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    Ok(())
}
pub fn request(home: &Path, method: &str, p: V) -> Result<V> {
    let endpoint: V = serde_json::from_slice(
        &fs::read(home.join(ENDPOINT)).context("Open herdr-bot or start herdr-bot serve first")?,
    )?;
    let port = endpoint["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .context("invalid host endpoint")?;
    let mut stream =
        TcpStream::connect_timeout(&(Ipv4Addr::LOCALHOST, port).into(), Duration::from_secs(3))
            .context("host is offline")?;
    stream.set_read_timeout(Some(Duration::from_secs(150)))?;
    serde_json::to_writer(
        &mut stream,
        &json!({"token":endpoint["token"],"method":method,"params":p}),
    )?;
    stream.write_all(b"\n")?;
    let result: V = serde_json::from_reader(stream)?;
    if let Some(e) = result.get("error") {
        bail!("{}", e["message"].as_str().unwrap_or("host error"))
    }
    Ok(result["value"].clone())
}
