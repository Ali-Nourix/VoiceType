use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
    pub language: String,
    pub source: String,
    pub startup: bool,
    pub auto_type: bool,
    pub normalize_persian: bool,
    pub hold_to_talk: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            ctrl: false,
            alt: true,
            shift: false,
            key: "Z".into(),
            language: "fa-IR".into(),
            source: "Microphone".into(),
            startup: false,
            auto_type: true,
            normalize_persian: true,
            hold_to_talk: false,
        }
    }
}
fn path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("VoiceType")
        .join("settings.json")
}
pub fn load() -> Settings {
    fs::read(path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save(settings: &Settings) -> Result<(), String> {
    let p = path();
    fs::create_dir_all(p.parent().ok_or("Invalid config path")?).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(p, bytes).map_err(|e| e.to_string())
}
#[cfg(target_os = "windows")]
pub fn set_startup(enabled: bool) -> Result<(), String> {
    use winreg::{enums::*, RegKey};
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = root
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|e| e.to_string())?;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        key.set_value("VoiceType", &format!("\"{}\" --minimized", exe.display()))
            .map_err(|e| e.to_string())
    } else {
        match key.delete_value("VoiceType") {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
