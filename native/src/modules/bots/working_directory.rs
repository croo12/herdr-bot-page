use crate::infrastructure::paths::expand;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub(super) fn resolve_working_directory(
    directory: Option<&Path>,
    allow_home: bool,
) -> Result<PathBuf> {
    let raw = directory.and_then(Path::to_str).unwrap_or_default();
    if raw.is_empty() {
        bail!("missing_cwd: choose a working folder in Settings or Advanced setup")
    }
    let cwd = expand(raw);
    if !cwd.is_dir() {
        bail!("missing_cwd: {}", cwd.display())
    }
    if !allow_home
        && cwd.canonicalize().ok() == dirs::home_dir().and_then(|home| home.canonicalize().ok())
    {
        bail!("home_cwd_requires_confirmation: confirm the home folder")
    }
    Ok(cwd)
}
