//! Presence detectors that drive the "auto-detect" toggles in the wizard.
//!
//! Each detector answers three questions cheaply (no full walk): is it
//! installed, where does its config live, and roughly how big is it. Size is
//! sampled to a bounded depth so opening the wizard stays instant even when a
//! folder holds 40 GB of model weights.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub id: String,
    pub name: String,
    pub found: bool,
    pub paths: Vec<String>,
    pub approx_bytes: u64,
    pub detail: Option<String>,
}

fn sample_size(p: &Path, max_depth: usize, cap: u64) -> u64 {
    let mut total = 0u64;
    for e in walkdir::WalkDir::new(p)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
    {
        if let Ok(m) = e.metadata() {
            if m.is_file() {
                total += m.len();
                if total >= cap {
                    return total;
                }
            }
        }
    }
    total
}

fn probe(id: &str, name: &str, candidates: &[PathBuf]) -> Detection {
    let paths: Vec<PathBuf> = candidates.iter().filter(|p| p.exists()).cloned().collect();
    let approx = paths.iter().map(|p| sample_size(p, 4, 4 << 30)).sum();
    Detection {
        id: id.into(),
        name: name.into(),
        found: !paths.is_empty(),
        paths: paths.iter().map(|p| p.display().to_string()).collect(),
        approx_bytes: approx,
        detail: None,
    }
}

fn appdata(rel: &str) -> PathBuf {
    dirs::config_dir().unwrap_or_default().join(rel)
}
fn local(rel: &str) -> PathBuf {
    dirs::data_local_dir().unwrap_or_default().join(rel)
}
fn home(rel: &str) -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(rel)
}

/// Editors, AI tools and local model runtimes.
pub fn ai_and_dev_tools() -> Vec<Detection> {
    let mut out = vec![
        probe(
            "vscode",
            "Visual Studio Code",
            &[appdata("Code/User"), home(".vscode/extensions")],
        ),
        probe(
            "cursor",
            "Cursor",
            &[appdata("Cursor/User"), home(".cursor")],
        ),
        probe("windsurf", "Windsurf", &[appdata("Windsurf/User"), home(".windsurf")]),
        probe("claude-desktop", "Claude Desktop", &[appdata("Claude")]),
        probe("chatgpt-desktop", "ChatGPT Desktop", &[appdata("ChatGPT")]),
        probe("jetbrains", "JetBrains IDEs", &[appdata("JetBrains")]),
        probe("continue", "Continue.dev", &[home(".continue")]),
        probe("zed", "Zed", &[appdata("Zed"), local("Zed")]),
    ];

    // Ollama: report weights separately so the wizard can show why they're
    // excluded by default.
    let ollama_root = home(".ollama");
    let blobs = ollama_root.join("models").join("blobs");
    let mut ollama = probe("ollama", "Ollama", &[ollama_root.clone()]);
    if ollama.found {
        let weights = sample_size(&blobs, 2, 512 << 30);
        ollama.detail = Some(format!(
            "{} of model weights are excluded by default — re-pull them with `ollama pull`. \
             Manifests are backed up so you keep the list.",
            crate::util::human_bytes(weights)
        ));
        ollama.approx_bytes = ollama.approx_bytes.saturating_sub(weights);
    }
    out.push(ollama);

    let lms = local("LM-Studio");
    let lms_cache = home(".cache/lm-studio");
    let mut lm = probe("lm-studio", "LM Studio", &[lms, lms_cache]);
    if lm.found {
        lm.detail =
            Some("Downloaded GGUF models are excluded by default; settings and chats are kept.".into());
    }
    out.push(lm);

    out
}

/// Everything the wizard shows, in display order.
pub fn all() -> Vec<Detection> {
    let mut out = vec![
        probe("desktop", "Desktop", &[home("Desktop")]),
        probe(
            "documents",
            "Documents / Pictures / Videos",
            &[home("Documents"), home("Pictures"), home("Videos")],
        ),
        probe(
            "opera-gx",
            "Opera GX",
            &[
                appdata("Opera Software/Opera GX Stable"),
                local("Opera Software/Opera GX Stable"),
            ],
        ),
        probe("chrome", "Google Chrome", &[local("Google/Chrome/User Data")]),
        probe("edge", "Microsoft Edge", &[local("Microsoft/Edge/User Data")]),
        probe("brave", "Brave", &[local("BraveSoftware/Brave-Browser/User Data")]),
        probe("firefox", "Firefox", &[appdata("Mozilla/Firefox")]),
        probe("wsl", "WSL config", &[home(".wslconfig")]),
        probe(
            "terminal",
            "Windows Terminal",
            &[local("Microsoft/Windows Terminal")],
        ),
        probe("ssh", "SSH keys", &[home(".ssh")]),
    ];

    // Steam needs the registry lookup, so it doesn't fit `probe`.
    if let Some(root) = crate::steam::install_dir() {
        out.push(Detection {
            id: "steam".into(),
            name: "Steam".into(),
            found: true,
            approx_bytes: sample_size(&root.join("config"), 2, 1 << 30)
                + sample_size(&root.join("userdata"), 5, 8 << 30),
            paths: vec![root.display().to_string()],
            detail: Some("Installed games are never backed up — only config and userdata.".into()),
        });
    } else {
        out.push(Detection {
            id: "steam".into(),
            name: "Steam".into(),
            found: false,
            paths: vec![],
            approx_bytes: 0,
            detail: None,
        });
    }

    out.extend(ai_and_dev_tools());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_missing_paths_as_not_found() {
        let d = probe("nope", "Nope", &[PathBuf::from("/definitely/not/here/prb")]);
        assert!(!d.found);
        assert_eq!(d.approx_bytes, 0);
        assert!(d.paths.is_empty());
    }

    #[test]
    fn every_detection_has_a_stable_id() {
        let all = all();
        let mut ids: Vec<&str> = all.iter().map(|d| d.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len());
    }
}
