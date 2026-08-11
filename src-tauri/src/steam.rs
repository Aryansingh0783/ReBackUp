//! Steam account/session discovery.
//!
//! # What is worth backing up
//! | File | Why |
//! |---|---|
//! | `config/loginusers.vdf` | account list + "remember me" flags |
//! | `config/config.vdf` | library folders, misc client config |
//! | `ssfn*` (in the Steam root) | Steam Guard sentry files |
//! | `%LOCALAPPDATA%\Steam\local.vdf` | machine-local client state |
//! | `userdata/<id3>/` | per-account settings, screenshots, cloud-less saves |
//!
//! # The SID caveat — read this before trusting `ssfn*`
//! Sentry files authorise a *machine + Windows account* pair. After a clean
//! install your user SID is different, so Steam will almost always demand a
//! fresh Steam Guard code even with the `ssfn*` files restored. Backing them up
//! costs nothing and occasionally works (same-SID reinstall, e.g. an in-place
//! repair), but **plan on re-authenticating**. What you reliably keep is the
//! account list, library layout and per-game settings under `userdata/`.

use crate::error::AppResult;
use crate::vdf;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SteamAccount {
    pub steam_id64: String,
    pub steam_id3: u32,
    pub account_name: String,
    pub persona_name: String,
    pub remember_password: bool,
    pub most_recent: bool,
    pub last_login: i64,
    pub userdata_dir: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SteamReport {
    pub install_dir: Option<String>,
    pub accounts: Vec<SteamAccount>,
    pub sentry_files: Vec<String>,
    pub library_folders: Vec<String>,
    pub config_files: Vec<String>,
    pub warnings: Vec<String>,
}

/// Registry first, then the usual install locations.
pub fn install_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                let p = PathBuf::from(p.replace('/', "\\"));
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let candidates = [
        r"C:\Program Files (x86)\Steam",
        r"C:\Program Files\Steam",
        r"D:\Steam",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.join("steam.exe").exists() || p.join("config").is_dir() {
            return Some(p);
        }
    }
    // Linux/macOS, for the cross-platform build.
    dirs::home_dir().and_then(|h| {
        [".steam/steam", ".local/share/Steam", "Library/Application Support/Steam"]
            .iter()
            .map(|s| h.join(s))
            .find(|p| p.is_dir())
    })
}

/// SteamID64 -> SteamID3 (the number used for `userdata/<id>` folders).
pub fn id64_to_id3(id64: &str) -> Option<u32> {
    let n: u64 = id64.parse().ok()?;
    // 0x0110_0001_0000_0000 is the individual-account base.
    n.checked_sub(76_561_197_960_265_728).map(|v| v as u32)
}

pub fn detect() -> AppResult<SteamReport> {
    let mut report = SteamReport::default();

    let Some(root) = install_dir() else {
        report
            .warnings
            .push("Steam does not appear to be installed for this user.".into());
        return Ok(report);
    };
    report.install_dir = Some(root.display().to_string());

    // --- accounts --------------------------------------------------------
    let loginusers = root.join("config").join("loginusers.vdf");
    if loginusers.exists() {
        report.config_files.push(loginusers.display().to_string());
        match std::fs::read_to_string(&loginusers) {
            Ok(text) => match vdf::parse(&text) {
                Ok(v) => {
                    if let Some(users) = v.get("users").and_then(vdf::Value::as_obj) {
                        for (id64, u) in users {
                            let id3 = id64_to_id3(id64).unwrap_or(0);
                            let udir = root.join("userdata").join(id3.to_string());
                            report.accounts.push(SteamAccount {
                                steam_id64: id64.clone(),
                                steam_id3: id3,
                                account_name: sget(u, "AccountName"),
                                persona_name: sget(u, "PersonaName"),
                                remember_password: sget(u, "RememberPassword") == "1",
                                most_recent: sget(u, "MostRecent") == "1",
                                last_login: sget(u, "Timestamp").parse().unwrap_or(0),
                                userdata_dir: udir.is_dir().then(|| udir.display().to_string()),
                            });
                        }
                    }
                }
                Err(e) => report.warnings.push(format!("loginusers.vdf: {e}")),
            },
            Err(e) => report.warnings.push(format!("loginusers.vdf: {e}")),
        }
    }

    // --- sentry files ----------------------------------------------------
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("ssfn") {
                report.sentry_files.push(e.path().display().to_string());
            }
        }
    }
    if report.sentry_files.is_empty() {
        report
            .warnings
            .push("No ssfn* sentry files found — Steam Guard will require a code after restore.".into());
    } else {
        report.warnings.push(
            "Sentry (ssfn*) files are bound to this machine AND this Windows user SID. After a \
             clean install the SID changes, so expect to re-enter a Steam Guard code even with \
             them restored."
                .into(),
        );
    }

    // --- other config ----------------------------------------------------
    for rel in ["config/config.vdf", "config/libraryfolders.vdf", "steamapps/libraryfolders.vdf"] {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if p.exists() {
            report.config_files.push(p.display().to_string());
        }
    }
    if let Some(local) = local_vdf() {
        report.config_files.push(local.display().to_string());
    }
    report.library_folders = library_folders(&root);

    Ok(report)
}

fn sget(v: &vdf::Value, k: &str) -> String {
    v.get(k).and_then(vdf::Value::as_str).unwrap_or("").to_string()
}

/// `%LOCALAPPDATA%\Steam\local.vdf`
pub fn local_vdf() -> Option<PathBuf> {
    let p = dirs::data_local_dir()?.join("Steam").join("local.vdf");
    p.exists().then_some(p)
}

/// Every install root listed in `libraryfolders.vdf`, including the base one.
pub fn library_folders(root: &Path) -> Vec<String> {
    let mut out = vec![root.display().to_string()];
    for rel in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let Ok(v) = vdf::parse(&text) else { continue };
        let Some(folders) = v.get("libraryfolders").and_then(vdf::Value::as_obj) else { continue };
        for (_, entry) in folders {
            // Modern format: { "path" "D:\\SteamLibrary" ... }. Old: "1" "D:\\..."
            let path = entry
                .get("path")
                .and_then(vdf::Value::as_str)
                .or_else(|| entry.as_str());
            if let Some(p) = path {
                let p = p.replace("\\\\", "\\");
                if !out.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Concrete file list for the Steam backup profile.
pub fn backup_targets(report: &SteamReport) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    v.extend(report.sentry_files.iter().map(PathBuf::from));
    v.extend(report.config_files.iter().map(PathBuf::from));
    v.extend(
        report
            .accounts
            .iter()
            .filter_map(|a| a.userdata_dir.as_ref())
            .map(PathBuf::from),
    );
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_steamid64_to_the_userdata_folder_number() {
        assert_eq!(id64_to_id3("76561197960265728"), Some(0));
        assert_eq!(id64_to_id3("76561198012345678"), Some(52_079_950));
        assert_eq!(id64_to_id3("not a number"), None);
    }

    #[test]
    fn tolerates_ids_below_the_individual_base() {
        assert_eq!(id64_to_id3("1"), None);
    }
}
