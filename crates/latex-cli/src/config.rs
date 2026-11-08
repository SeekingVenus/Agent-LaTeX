//! Persistent user settings.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle_key: Option<String>,
}

/// `$XDG_CONFIG_HOME/agent-latex/config.json`, or
/// `~/.config/agent-latex/config.json`.
pub fn path() -> PathBuf {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(value).join("agent-latex").join("config.json");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("agent-latex")
            .join("config.json");
    }
    PathBuf::from(".agent-latex").join("config.json")
}

pub fn load() -> Result<Config> {
    load_from(&path())
}

fn load_from(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn set_toggle_key(value: &str) -> Result<PathBuf> {
    let path = path();
    set_key_at(&path, "toggle_key", value)?;
    Ok(path)
}

// shortcut: concurrent `config set` processes are last-writer-wins. The only
// setting today is one hotkey, so a collision is harmless. Add a file lock when
// more settings make lost updates possible.
fn set_key_at(path: &Path, key: &str, value: &str) -> Result<()> {
    let mut root = if path.exists() {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str::<serde_json::Value>(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?
    } else {
        serde_json::json!({})
    };
    let Some(object) = root.as_object_mut() else {
        bail!("{} must contain a JSON object", path.display());
    };
    object.insert(
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    {
        let mut file = fs::File::create(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        file.write_all(serde_json::to_string_pretty(&root)?.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}
