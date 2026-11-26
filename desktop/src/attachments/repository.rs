use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};
pub(super) fn source(home: &Path, path: &str) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    let dir = fs::canonicalize(home.join("attachments")).ok()?;
    (path.is_file() && path.starts_with(dir)).then_some(path)
}
pub(super) enum FileBytes {
    TooLarge(u64),
    Data(Vec<u8>),
}

pub(super) fn read_bounded(path: &Path, limit: u64) -> io::Result<FileBytes> {
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > limit {
        return Ok(FileBytes::TooLarge(size));
    }
    let mut data = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Ok(FileBytes::TooLarge(data.len() as u64));
    }
    Ok(FileBytes::Data(data))
}

pub(super) fn discard(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

pub(super) fn copy_to(source: &Path, destination: &Path) -> io::Result<u64> {
    fs::copy(source, destination)
}

pub(crate) fn data_url(path: &Path, limit: u64) -> Option<String> {
    let FileBytes::Data(data) = read_bounded(path, limit).ok()? else {
        return None;
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(format!("data:{mime};base64,{}", STANDARD.encode(data)))
}
pub(super) fn stage(home: &Path, filename: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let dir = home.join("attachments");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}-{filename}", uuid::Uuid::new_v4()));
    fs::write(&path, bytes)?;
    Ok(path)
}
pub(super) fn read_prefix(path: &Path, limit: u64) -> io::Result<(u64, Vec<u8>)> {
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut data = Vec::new();
    file.take(limit).read_to_end(&mut data)?;
    Ok((size, data))
}
