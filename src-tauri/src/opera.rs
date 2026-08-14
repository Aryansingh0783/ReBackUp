//! Browser profile detection — Opera GX first, then the rest of the Chromium
//! family, plus Firefox (detected but handled differently).
//!
//! # Opera's layout differs from Chrome's
//! Chrome:   `...\User Data\Local State` + `...\User Data\Default\Login Data`
//! Opera GX: `%APPDATA%\Opera Software\Opera GX Stable\` holds **both**
//!           `Local State` and `Login Data` in the same directory, and
//!           `%LOCALAPPDATA%\Opera Software\Opera GX Stable\` holds the cache.
//! Getting this wrong is the single most common reason a "Chrome password
//! decryptor" fails on Opera, so `find_local_state` checks the profile dir
//! first and only then walks up.
//!
//! # Why there is no `--remote-debugging-port` automation here
//! The spec floated driving the browser's Export button over CDP. We don't:
//! opening a debug port makes *every* process on the machine able to drive the
//! browser and read its cookies for as long as it's open. Direct DPAPI
//! decryption gets the same data without that exposure, and when the profile
//! uses app-bound encryption we hand the user a guided manual export instead.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Chromium,
    Gecko,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    /// e.g. `Opera GX`
    pub browser: String,
    /// e.g. `Default`, or the Opera profile folder name.
    pub profile: String,
    pub engine: Engine,
    /// Directory holding `Login Data` (Chromium) or `logins.json` (Gecko).
    pub data_dir: String,
    /// Directory holding `Local State` (Chromium only).
    pub local_state: Option<String>,
    /// Roaming + local directories that the file backup should copy wholesale.
    pub backup_dirs: Vec<String>,
    pub has_login_db: bool,
    pub app_bound: bool,
    pub size_hint_bytes: u64,
    pub notes: Vec<String>,
}

struct Candidate {
    browser: &'static str,
    /// `true` = roaming appdata (%APPDATA%), `false` = local (%LOCALAPPDATA%).
    roaming: bool,
    rel: &'static str,
}

/// Find the directories that actually hold `Login Data` under `root`.
///
/// Layout is **detected, not assumed**. Older Opera keeps `Login Data` flat in
/// the profile root; current Opera GX has migrated to Chrome's `Default\`
/// subfolder while leaving `Local State` in the parent. Hard-coding either one
/// silently reports "no saved passwords" on half the installs out there — which
/// is the single worst way for this tool to fail, because the user finds out
/// after the disk is wiped.
fn discover_profiles(root: &Path) -> Vec<(String, PathBuf)> {
    // 1. Flat layout: Login Data sits directly in the root.
    if root.join("Login Data").is_file() {
        return vec![("Default".to_string(), root.to_path_buf())];
    }

    // 2. Chrome layout: Default\ and Profile N\ subfolders.
    let mut found = Vec::new();
    if root.join("Default").is_dir() {
        found.push(("Default".to_string(), root.join("Default")));
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        let mut extra: Vec<(String, PathBuf)> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .filter(|(n, _)| n.starts_with("Profile "))
            .collect();
        extra.sort_by(|a, b| a.0.cmp(&b.0));
        found.extend(extra);
    }
    if !found.is_empty() {
        return found;
    }

    // 3. Neither: report the root so the UI still shows the browser exists.
    vec![("Default".to_string(), root.to_path_buf())]
}

const CHROMIUM: &[Candidate] = &[
    Candidate {
        browser: "Opera GX",
        roaming: true,
        rel: r"Opera Software\Opera GX Stable",
    },
    Candidate {
        browser: "Opera",
        roaming: true,
        rel: r"Opera Software\Opera Stable",
    },
    Candidate {
        browser: "Opera Air",
        roaming: true,
        rel: r"Opera Software\Opera Air Stable",
    },
    Candidate {
        browser: "Google Chrome",
        roaming: false,
        rel: r"Google\Chrome\User Data",
    },
    Candidate {
        browser: "Microsoft Edge",
        roaming: false,
        rel: r"Microsoft\Edge\User Data",
    },
    Candidate {
        browser: "Brave",
        roaming: false,
        rel: r"BraveSoftware\Brave-Browser\User Data",
    },
    Candidate {
        browser: "Vivaldi",
        roaming: false,
        rel: r"Vivaldi\User Data",
    },
    Candidate {
        browser: "Chromium",
        roaming: false,
        rel: r"Chromium\User Data",
    },
];

fn base(roaming: bool) -> Option<PathBuf> {
    if roaming {
        dirs::config_dir() // %APPDATA% on Windows
    } else {
        dirs::data_local_dir() // %LOCALAPPDATA%
    }
}

/// `Local State` sits either in the profile dir (Opera) or its parent (Chrome).
pub fn find_local_state(profile_dir: &Path) -> Option<PathBuf> {
    let here = profile_dir.join("Local State");
    if here.is_file() {
        return Some(here);
    }
    let up = profile_dir.parent()?.join("Local State");
    up.is_file().then_some(up)
}

fn cheap_size(dir: &Path, cap: u64) -> u64 {
    let mut total = 0;
    for e in walkdir::WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
    {
        if let Ok(m) = e.metadata() {
            if m.is_file() {
                total += m.len();
                if total > cap {
                    return total;
                }
            }
        }
    }
    total
}

/// Every Chromium/Gecko profile we can find for the current user.
pub fn detect_all() -> Vec<BrowserProfile> {
    let mut out = Vec::new();

    for c in CHROMIUM {
        let Some(root) = base(c.roaming).map(|b| b.join(c.rel)) else {
            continue;
        };
        if !root.is_dir() {
            continue;
        }

        // The matching directory under the *other* appdata root, which holds
        // caches, extensions state and (for Opera) the GX-specific data.
        let sibling = base(!c.roaming)
            .map(|b| b.join(c.rel))
            .filter(|p| p.is_dir());

        let profile_dirs = discover_profiles(&root);

        for (profile, data_dir) in profile_dirs {
            let local_state = find_local_state(&data_dir);
            let app_bound = local_state
                .as_deref()
                .map(has_app_bound_marker)
                .unwrap_or(false);

            let mut backup_dirs = vec![root.display().to_string()];
            if let Some(s) = &sibling {
                backup_dirs.push(s.display().to_string());
            }

            let mut notes = Vec::new();
            if app_bound {
                notes.push(
                    "This profile uses app-bound encryption; saved passwords must be exported \
                     from the browser's own password manager."
                        .into(),
                );
            }
            if c.browser.starts_with("Opera") {
                notes.push(format!(
                    "Opera layout detected as {}: Login Data in {}, Local State in {}.",
                    if data_dir == root {
                        "flat"
                    } else {
                        "Chrome-style (Default\\)"
                    },
                    data_dir.display(),
                    local_state
                        .as_deref()
                        .and_then(|p| Path::new(p).parent().map(|d| d.display().to_string()))
                        .unwrap_or_else(|| "not found".into())
                ));
            }

            out.push(BrowserProfile {
                browser: c.browser.to_string(),
                profile,
                engine: Engine::Chromium,
                has_login_db: data_dir.join("Login Data").is_file(),
                data_dir: data_dir.display().to_string(),
                local_state: local_state.map(|p| p.display().to_string()),
                size_hint_bytes: cheap_size(&data_dir, 64 * 1024 * 1024 * 1024),
                backup_dirs,
                app_bound,
                notes,
            });
        }
    }

    out.extend(detect_firefox());
    out
}

fn has_app_bound_marker(local_state: &Path) -> bool {
    std::fs::read_to_string(local_state)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .is_some_and(|j| j.pointer("/os_crypt/app_bound_encrypted_key").is_some())
}

/// Firefox stores logins in `logins.json`, encrypted with an NSS key from
/// `key4.db`. We don't decrypt it — we copy the profile, which preserves the
/// logins as long as the user has no Primary Password.
fn detect_firefox() -> Vec<BrowserProfile> {
    let Some(root) = dirs::config_dir().map(|b| b.join(r"Mozilla\Firefox\Profiles")) else {
        return vec![];
    };
    let Ok(rd) = std::fs::read_dir(&root) else {
        return vec![];
    };

    rd.flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let dir = e.path();
            BrowserProfile {
                browser: "Firefox".into(),
                profile: e.file_name().to_string_lossy().into_owned(),
                engine: Engine::Gecko,
                has_login_db: dir.join("logins.json").is_file(),
                local_state: None,
                size_hint_bytes: cheap_size(&dir, 16 * 1024 * 1024 * 1024),
                backup_dirs: vec![dir.display().to_string()],
                app_bound: false,
                notes: vec![
                    "Firefox logins live in logins.json + key4.db. Copying the whole profile \
                     restores them intact; if you set a Primary Password you'll need it again."
                        .into(),
                ],
                data_dir: dir.display().to_string(),
            }
        })
        .collect()
}

/// Best-effort launch of the browser's own password settings page.
///
/// Chromium refuses `browser.exe <scheme>://settings` from the command line in
/// recent builds, so this opens the browser and the caller shows the URL for
/// the user to paste. We never claim it worked.
pub fn password_manager_url(browser: &str) -> &'static str {
    match browser {
        b if b.starts_with("Opera") => "opera://settings/passwords",
        "Microsoft Edge" => "edge://settings/passwords",
        "Brave" => "brave://settings/passwords",
        "Vivaldi" => "vivaldi://settings/passwords",
        "Firefox" => "about:logins",
        _ => "chrome://password-manager/passwords",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_local_state_inside_the_profile_dir() {
        let tmp = std::env::temp_dir().join(format!("rbu-op-{}", std::process::id()));
        let profile = tmp.join("Opera GX Stable");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(tmp.join("Local State"), "{}").unwrap();
        std::fs::write(profile.join("Local State"), "{}").unwrap();

        assert_eq!(
            find_local_state(&profile).unwrap(),
            profile.join("Local State")
        );
        std::fs::remove_file(profile.join("Local State")).unwrap();
        // Falls back to the parent, which is the Chrome layout.
        assert_eq!(find_local_state(&profile).unwrap(), tmp.join("Local State"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detects_both_opera_layouts() {
        let tmp = std::env::temp_dir().join(format!("rbu-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        // Flat: Login Data directly in the profile root (older Opera).
        let flat = tmp.join("flat");
        std::fs::create_dir_all(&flat).unwrap();
        std::fs::write(flat.join("Login Data"), b"x").unwrap();
        let p = discover_profiles(&flat);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].1, flat);

        // Chrome-style: Default\Login Data with Local State in the parent.
        // Current Opera GX ships this layout; assuming "flat" here is what made
        // a real install report "no saved passwords".
        let chromed = tmp.join("chromed");
        std::fs::create_dir_all(chromed.join("Default")).unwrap();
        std::fs::create_dir_all(chromed.join("Profile 2")).unwrap();
        std::fs::write(chromed.join("Local State"), "{}").unwrap();
        std::fs::write(chromed.join("Default").join("Login Data"), b"x").unwrap();
        let p = discover_profiles(&chromed);
        assert_eq!(p.len(), 2, "Default + Profile 2");
        assert_eq!(p[0].1, chromed.join("Default"));
        assert_eq!(
            find_local_state(&p[0].1).unwrap(),
            chromed.join("Local State"),
            "Local State must be found in the parent for the Chrome layout"
        );

        // Neither: still report the root so the browser shows up in the UI.
        let empty = tmp.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(
            discover_profiles(&empty),
            vec![("Default".to_string(), empty)]
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn maps_browsers_to_their_settings_urls() {
        assert_eq!(
            password_manager_url("Opera GX"),
            "opera://settings/passwords"
        );
        assert_eq!(
            password_manager_url("Google Chrome"),
            "chrome://password-manager/passwords"
        );
        assert_eq!(password_manager_url("Firefox"), "about:logins");
    }
}
