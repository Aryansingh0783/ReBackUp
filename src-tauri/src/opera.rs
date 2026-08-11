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
    /// (base dir kind, relative path). `true` = roaming appdata, `false` = local.
    roaming: bool,
    rel: &'static str,
    /// Chrome-style: profiles are subfolders and Local State is the parent.
    chrome_style: bool,
}

const CHROMIUM: &[Candidate] = &[
    Candidate {
        browser: "Opera GX",
        roaming: true,
        rel: r"Opera Software\Opera GX Stable",
        chrome_style: false,
    },
    Candidate {
        browser: "Opera",
        roaming: true,
        rel: r"Opera Software\Opera Stable",
        chrome_style: false,
    },
    Candidate {
        browser: "Opera Air",
        roaming: true,
        rel: r"Opera Software\Opera Air Stable",
        chrome_style: false,
    },
    Candidate {
        browser: "Google Chrome",
        roaming: false,
        rel: r"Google\Chrome\User Data",
        chrome_style: true,
    },
    Candidate {
        browser: "Microsoft Edge",
        roaming: false,
        rel: r"Microsoft\Edge\User Data",
        chrome_style: true,
    },
    Candidate {
        browser: "Brave",
        roaming: false,
        rel: r"BraveSoftware\Brave-Browser\User Data",
        chrome_style: true,
    },
    Candidate {
        browser: "Vivaldi",
        roaming: false,
        rel: r"Vivaldi\User Data",
        chrome_style: true,
    },
    Candidate {
        browser: "Chromium",
        roaming: false,
        rel: r"Chromium\User Data",
        chrome_style: true,
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

        let profile_dirs: Vec<(String, PathBuf)> = if c.chrome_style {
            let mut v = Vec::new();
            if root.join("Default").is_dir() {
                v.push(("Default".to_string(), root.join("Default")));
            }
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with("Profile ") && e.path().is_dir() {
                        v.push((name, e.path()));
                    }
                }
            }
            v
        } else {
            vec![("Default".to_string(), root.clone())]
        };

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
                notes.push(
                    "Opera keeps `Local State` inside the profile folder, not its parent.".into(),
                );
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
        let tmp = std::env::temp_dir().join(format!("prb-op-{}", std::process::id()));
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
