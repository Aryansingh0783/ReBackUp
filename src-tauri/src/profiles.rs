//! Backup profiles: the declarative "what to take" layer.
//!
//! A profile is a named bundle of path patterns plus zero or more *secret
//! actions* (things that need decryption/re-encryption rather than a file
//! copy). Built-ins are compiled in; user edits and custom profiles are
//! persisted as JSON next to the app config so they survive upgrades.
//!
//! Paths use `%ENV%` placeholders and `**` globs, expanded at plan time — never
//! at definition time — so a profile file is portable between machines.

use crate::error::AppResult;
use crate::util::expand_env;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum SecretAction {
    /// Decrypt this Chromium profile's `Login Data` and seal it as a CSV.
    ChromiumPasswords,
    /// Inventory Credential Manager + guide the `.crd` export.
    WindowsVault,
    /// Seal `~/.git-credentials` (plaintext on disk) and private SSH keys.
    GitCredentials,
    /// Seal Steam sentry files (they authorise a session).
    SteamSentry,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Files,
    Browser,
    Games,
    Development,
    AiTools,
    System,
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    /// Glob patterns, `%ENV%`-expandable. A pattern with no wildcard is treated
    /// as a literal file or directory.
    pub include: Vec<String>,
    /// Applied to the expanded include set. Caches and reproducible junk.
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<SecretAction>,
    #[serde(default)]
    pub enabled_by_default: bool,
    /// Set at runtime by the detectors; not persisted meaningfully.
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl Profile {
    /// Expand `%ENV%` and `~`, dropping patterns whose root doesn't exist.
    pub fn expanded_includes(&self) -> Vec<String> {
        self.include.iter().map(|p| expand_env(p)).collect()
    }
    pub fn expanded_excludes(&self) -> Vec<String> {
        self.exclude.iter().map(|p| expand_env(p)).collect()
    }
}

/// Cache/junk that is never worth copying, applied on top of every profile.
pub const GLOBAL_EXCLUDES: &[&str] = &[
    "**/Cache/**",
    "**/Code Cache/**",
    "**/GPUCache/**",
    "**/ShaderCache/**",
    "**/GrShaderCache/**",
    "**/Service Worker/CacheStorage/**",
    "**/Crashpad/**",
    "**/CrashReports/**",
    "**/component_crx_cache/**",
    "**/*.tmp",
    "**/Thumbs.db",
    "**/desktop.ini",
    "**/node_modules/**",
    "**/.next/cache/**",
    "**/__pycache__/**",
];

pub fn builtin() -> Vec<Profile> {
    let p = |id: &str,
             name: &str,
             description: &str,
             category: Category,
             include: &[&str],
             exclude: &[&str],
             secrets: Vec<SecretAction>,
             enabled: bool,
             notes: &[&str]| Profile {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        category,
        include: include.iter().map(|s| s.to_string()).collect(),
        exclude: exclude.iter().map(|s| s.to_string()).collect(),
        secrets,
        enabled_by_default: enabled,
        builtin: true,
        notes: notes.iter().map(|s| s.to_string()).collect(),
    };

    vec![
        p(
            "desktop",
            "Desktop files",
            "Everything sitting on your Desktop. Pick individual items in the review step.",
            Category::Files,
            &["%USERPROFILE%/Desktop/**"],
            &["%USERPROFILE%/Desktop/desktop.ini"],
            vec![],
            true,
            &["Shortcuts (.lnk) are copied but will point at paths that no longer exist."],
        ),
        p(
            "documents",
            "Documents, Pictures, Videos",
            "The standard user folders, minus OneDrive placeholders.",
            Category::Files,
            &[
                "%USERPROFILE%/Documents/**",
                "%USERPROFILE%/Pictures/**",
                "%USERPROFILE%/Videos/**",
                "%USERPROFILE%/Music/**",
            ],
            &["**/OneDriveTemp/**"],
            vec![],
            false,
            &["Files marked 'online-only' by OneDrive will be downloaded when copied."],
        ),
        p(
            "opera-gx",
            "Opera GX",
            "Opera GX profile: bookmarks, sessions, extensions, settings, and saved passwords.",
            Category::Browser,
            &[
                "%APPDATA%/Opera Software/Opera GX Stable/**",
                "%LOCALAPPDATA%/Opera Software/Opera GX Stable/**",
            ],
            &[
                "**/Opera GX Stable/Cache/**",
                "**/Opera GX Stable/Code Cache/**",
                "**/Opera GX Stable/GPUCache/**",
            ],
            vec![SecretAction::ChromiumPasswords],
            true,
            &[
                "Passwords are decrypted via DPAPI and re-sealed with your passphrase — the \
                 profile's own copy stays encrypted to the old Windows account and is useless \
                 after the reset.",
                "Restoring the profile folder brings back tabs, sessions and extensions; \
                 passwords come back via the sealed CSV import.",
            ],
        ),
        p(
            "chromium-browsers",
            "Chrome / Edge / Brave / Vivaldi",
            "Other Chromium profiles found on this machine.",
            Category::Browser,
            &[
                "%LOCALAPPDATA%/Google/Chrome/User Data/**",
                "%LOCALAPPDATA%/Microsoft/Edge/User Data/**",
                "%LOCALAPPDATA%/BraveSoftware/Brave-Browser/User Data/**",
                "%LOCALAPPDATA%/Vivaldi/User Data/**",
            ],
            &["**/Cache/**", "**/Code Cache/**", "**/GPUCache/**"],
            vec![SecretAction::ChromiumPasswords],
            false,
            &[],
        ),
        p(
            "firefox",
            "Firefox",
            "Firefox profiles. Copied wholesale — logins.json + key4.db travel together.",
            Category::Browser,
            &["%APPDATA%/Mozilla/Firefox/**"],
            &["**/cache2/**", "**/startupCache/**"],
            vec![],
            false,
            &["Only works if you don't use a Primary Password you've forgotten."],
        ),
        p(
            "steam",
            "Steam",
            "Account list, sentry files, library layout and per-account userdata.",
            Category::Games,
            &[
                "%PROGRAMFILES(X86)%/Steam/config/loginusers.vdf",
                "%PROGRAMFILES(X86)%/Steam/config/config.vdf",
                "%PROGRAMFILES(X86)%/Steam/ssfn*",
                "%PROGRAMFILES(X86)%/Steam/userdata/**",
                "%LOCALAPPDATA%/Steam/local.vdf",
            ],
            &["**/userdata/**/730/**/screenshots/thumbnails/**"],
            vec![SecretAction::SteamSentry],
            true,
            &[
                "ssfn* sentry files are bound to this machine AND this Windows user SID. Expect \
                 to re-enter a Steam Guard code after the reset.",
                "Installed games are NOT backed up — re-download them. Only saves outside Steam \
                 Cloud need copying.",
            ],
        ),
        p(
            "git-repos",
            "Git repositories",
            "Repos with unpushed or uncommitted work, plus remotes, config and SSH keys.",
            Category::Development,
            &[
                "%USERPROFILE%/.gitconfig",
                "%USERPROFILE%/.ssh/**",
            ],
            &[],
            vec![SecretAction::GitCredentials],
            true,
            &[
                "Repo *contents* are added by the scanner, not by this pattern — run the Git \
                 discovery step so only repos that need it get copied.",
            ],
        ),
        p(
            "windows-credentials",
            "Windows Credentials",
            "Credential Manager inventory plus a guided .crd export.",
            Category::System,
            &[],
            &[],
            vec![SecretAction::WindowsVault],
            true,
            &[
                "The .crd export runs on the Ctrl+Alt+Del secure desktop and cannot be \
                 automated — by design.",
            ],
        ),
        p(
            "ai-tools",
            "AI & editor tools",
            "Cursor, VS Code, Claude Desktop, ChatGPT, Ollama and LM Studio settings.",
            Category::AiTools,
            &[
                "%APPDATA%/Code/User/settings.json",
                "%APPDATA%/Code/User/keybindings.json",
                "%APPDATA%/Code/User/snippets/**",
                "%USERPROFILE%/.vscode/extensions/**",
                "%APPDATA%/Cursor/User/settings.json",
                "%APPDATA%/Cursor/User/keybindings.json",
                "%USERPROFILE%/.cursor/**",
                "%APPDATA%/Claude/**",
                "%APPDATA%/ChatGPT/**",
                "%USERPROFILE%/.ollama/models/manifests/**",
                "%USERPROFILE%/.cache/lm-studio/**",
                "%USERPROFILE%/.continue/**",
                "%APPDATA%/JetBrains/**",
            ],
            &[
                "**/logs/**",
                "**/Cache/**",
                "**/CachedData/**",
                "**/.ollama/models/blobs/**",
            ],
            vec![],
            true,
            &[
                "Ollama/LM Studio model *weights* are excluded — they're tens of GB and \
                 re-downloadable. Manifests are kept so `ollama pull` can restore the list.",
                "MCP server configs under %APPDATA%/Claude may contain API keys; they are \
                 sealed rather than copied in the clear.",
            ],
        ),
        p(
            "app-configs",
            "App settings",
            "Terminal, PowerShell, WSL and shell dotfiles.",
            Category::System,
            &[
                "%LOCALAPPDATA%/Packages/Microsoft.WindowsTerminal_*/LocalState/settings.json",
                "%USERPROFILE%/Documents/PowerShell/**",
                "%USERPROFILE%/Documents/WindowsPowerShell/**",
                "%USERPROFILE%/.wslconfig",
                "%USERPROFILE%/.bashrc",
                "%USERPROFILE%/.zshrc",
                "%USERPROFILE%/.npmrc",
                "%USERPROFILE%/.condarc",
                "%APPDATA%/pip/pip.ini",
            ],
            &[],
            vec![],
            false,
            &[".npmrc and pip.ini frequently contain registry auth tokens; they are sealed."],
        ),
        p(
            "custom",
            "Custom paths",
            "Your own glob patterns. Nothing is included until you add some.",
            Category::Custom,
            &[],
            &[],
            vec![],
            false,
            &[],
        ),
    ]
}

/// `%APPDATA%\pre-reset-backup\profiles.json`
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pre-reset-backup")
        .join("profiles.json")
}

/// Built-ins overlaid with the user's saved copy (matched by `id`), plus any
/// custom profiles they added.
pub fn load() -> Vec<Profile> {
    let mut merged = builtin();
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return merged;
    };
    let Ok(saved) = serde_json::from_str::<Vec<Profile>>(&text) else {
        tracing::warn!("profiles.json is unreadable; falling back to built-ins");
        return merged;
    };
    for s in saved {
        match merged.iter_mut().find(|m| m.id == s.id) {
            Some(m) => {
                // Preserve the built-in description/notes, take the user's
                // selection and patterns.
                m.include = s.include;
                m.exclude = s.exclude;
                m.enabled_by_default = s.enabled_by_default;
                if !s.secrets.is_empty() {
                    m.secrets = s.secrets;
                }
            }
            None => merged.push(Profile { builtin: false, ..s }),
        }
    }
    merged
}

pub fn save(profiles: &[Profile]) -> AppResult<()> {
    let path = config_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(profiles)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_has_a_unique_id() {
        let b = builtin();
        let mut ids: Vec<&str> = b.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate profile id");
    }

    #[test]
    fn opera_profile_covers_both_appdata_roots() {
        let b = builtin();
        let opera = b.iter().find(|p| p.id == "opera-gx").unwrap();
        assert!(opera.include.iter().any(|i| i.contains("APPDATA")));
        assert!(opera.include.iter().any(|i| i.contains("LOCALAPPDATA")));
        assert!(opera.secrets.contains(&SecretAction::ChromiumPasswords));
    }

    #[test]
    fn env_expansion_leaves_globs_intact() {
        std::env::set_var("PRB_TEST_HOME", r"C:\Users\test");
        let p = Profile {
            id: "t".into(),
            name: "t".into(),
            description: String::new(),
            category: Category::Custom,
            include: vec!["%PRB_TEST_HOME%/Desktop/**".into()],
            exclude: vec![],
            secrets: vec![],
            enabled_by_default: false,
            builtin: false,
            notes: vec![],
        };
        assert_eq!(p.expanded_includes()[0], r"C:\Users\test/Desktop/**");
    }

    #[test]
    fn model_weights_are_excluded_from_the_ai_profile() {
        let b = builtin();
        let ai = b.iter().find(|p| p.id == "ai-tools").unwrap();
        assert!(ai.exclude.iter().any(|e| e.contains("blobs")));
    }
}
