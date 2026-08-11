//! Git repository discovery and credential-helper inspection.
//!
//! The goal is *not* to back up working trees — those are usually large and
//! reproducible from a remote. The goal is to make sure nothing is lost:
//! * repos with **unpushed commits** or **uncommitted changes** (these are the
//!   ones you actually have to copy),
//! * every remote URL, so the fresh install can re-clone,
//! * which credential helper each repo uses, so you know what to reconfigure.
//!
//! Credential *values* are never exported here. `manager`/`wincred` store their
//! secrets in Windows Credential Manager, which is covered by
//! [`crate::secrets::vault`]; `store` keeps a plaintext `~/.git-credentials`,
//! which we flag loudly and seal rather than copy in the clear.

use crate::error::AppResult;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    pub url: String,
    /// `https`, `ssh`, `git`, `file` or `other` — drives the restore hints.
    pub scheme: &'static str,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub path: String,
    pub branch: Option<String>,
    pub remotes: Vec<Remote>,
    pub credential_helper: Option<String>,
    pub dirty: Option<bool>,
    pub ahead: Option<u32>,
    pub untracked: Option<u32>,
    pub bare: bool,
    pub worktree_bytes: u64,
    /// True when there is no remote at all, or there are unpushed/uncommitted
    /// changes — i.e. the repo cannot be recreated by cloning.
    pub must_back_up: bool,
    pub notes: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitReport {
    pub repos: Vec<RepoInfo>,
    pub global_credential_helper: Option<String>,
    pub git_credentials_file: Option<String>,
    pub global_config: Option<String>,
    pub ssh_keys: Vec<String>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// .git/config parsing (INI-ish, but with `[section "sub"]` headers)
// ---------------------------------------------------------------------------

pub fn parse_config(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let inner = &line[1..line.len() - 1];
            section = match inner.split_once(char::is_whitespace) {
                Some((s, sub)) => format!("{}.{}", s.trim(), sub.trim().trim_matches('"')),
                None => inner.trim().to_string(),
            }
            .to_lowercase();
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = format!("{section}.{}", k.trim().to_lowercase());
            // Later definitions win, matching git's own last-one-wins rule.
            out.insert(key, v.trim().trim_matches('"').to_string());
        }
    }
    out
}

fn scheme_of(url: &str) -> &'static str {
    let u = url.to_lowercase();
    if u.starts_with("https://") || u.starts_with("http://") {
        "https"
    } else if u.starts_with("ssh://") || u.contains('@') && u.contains(':') && !u.contains("://") {
        "ssh"
    } else if u.starts_with("git://") {
        "git"
    } else if u.starts_with("file://") || u.starts_with('/') || u.chars().nth(1) == Some(':') {
        "file"
    } else {
        "other"
    }
}

fn read_branch(git_dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    head.strip_prefix("ref: refs/heads/")
        .map(str::to_string)
        // Detached HEAD: show the short sha instead.
        .or_else(|| Some(format!("(detached {})", &head[..head.len().min(8)])))
}

/// `git status` + `rev-list` in one shot. Returns `None` if git isn't on PATH.
fn probe_status(work_tree: &Path) -> Option<(bool, u32, u32)> {
    let out = run_git(work_tree, &["status", "--porcelain=v1", "--branch"])?;
    let mut dirty = false;
    let mut untracked = 0u32;
    let mut ahead = 0u32;
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(i) = rest.find("[ahead ") {
                ahead = rest[i + 7..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
            }
            continue;
        }
        if line.starts_with("?? ") {
            untracked += 1;
        } else if !line.trim().is_empty() {
            dirty = true;
        }
    }
    Some((dirty, ahead, untracked))
}

/// Run git with a hard timeout so a hung credential prompt can't wedge a scan.
fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // Belt and braces: never let a helper pop a GUI prompt mid-scan.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_OPTIONAL_LOCKS", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().ok()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn dir_size(path: &Path, cap: u64) -> u64 {
    let mut total = 0u64;
    for e in walkdir::WalkDir::new(path)
        .max_depth(12)
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

/// Inspect one `.git` directory (or a bare repo directory).
pub fn inspect(git_dir: &Path, run_status: bool) -> RepoInfo {
    let bare = git_dir.file_name().is_some_and(|n| n != ".git");
    let work_tree = if bare {
        git_dir.to_path_buf()
    } else {
        git_dir.parent().unwrap_or(git_dir).to_path_buf()
    };

    let cfg = std::fs::read_to_string(git_dir.join("config"))
        .map(|s| parse_config(&s))
        .unwrap_or_default();

    let mut remotes: Vec<Remote> = cfg
        .iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("remote.")
                .and_then(|rest| rest.strip_suffix(".url"))
                .map(|name| Remote {
                    name: name.to_string(),
                    url: v.clone(),
                    scheme: scheme_of(v),
                })
        })
        .collect();
    remotes.sort_by(|a, b| a.name.cmp(&b.name));

    let credential_helper = cfg.get("credential.helper").cloned();
    let (dirty, ahead, untracked) = if run_status && !bare {
        match probe_status(&work_tree) {
            Some((d, a, u)) => (Some(d), Some(a), Some(u)),
            None => (None, None, None),
        }
    } else {
        (None, None, None)
    };

    let mut notes = Vec::new();
    if remotes.is_empty() {
        notes.push("No remote configured — this repo exists nowhere else. Back up the whole folder.".into());
    }
    if let Some(h) = &credential_helper {
        match h.as_str() {
            "store" => notes.push(
                "credential.helper=store keeps tokens in PLAINTEXT (~/.git-credentials). \
                 Rotate those tokens after the reset."
                    .into(),
            ),
            "wincred" => notes.push(
                "credential.helper=wincred stores secrets in Windows Credential Manager — \
                 covered by the Windows Credentials profile."
                    .into(),
            ),
            h if h.contains("manager") => notes.push(
                "Git Credential Manager stores secrets in Windows Credential Manager (or DPAPI). \
                 Re-authenticating after the reset is usually faster than restoring them."
                    .into(),
            ),
            _ => {}
        }
    }

    let must_back_up = remotes.is_empty()
        || dirty.unwrap_or(false)
        || ahead.unwrap_or(0) > 0
        || untracked.unwrap_or(0) > 0;

    RepoInfo {
        path: work_tree.display().to_string(),
        branch: read_branch(git_dir),
        remotes,
        credential_helper,
        dirty,
        ahead,
        untracked,
        bare,
        worktree_bytes: dir_size(&work_tree, 8 * 1024 * 1024 * 1024),
        must_back_up,
        notes,
    }
}

/// Walk `roots` looking for `.git` directories.
///
/// Prefer [`from_paths`] when a scan index is already loaded — it's free.
pub fn discover(roots: &[PathBuf], max_depth: usize, run_status: bool) -> AppResult<GitReport> {
    let mut report = base_report();
    for root in roots {
        for e in walkdir::WalkDir::new(root)
            .max_depth(max_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_noise(e.path()))
            .filter_map(Result::ok)
        {
            if e.file_type().is_dir() && e.file_name() == ".git" {
                report.repos.push(inspect(e.path(), run_status));
            }
        }
    }
    finish(&mut report);
    Ok(report)
}

/// Build a report from `.git` paths already known to the scanner.
pub fn from_paths(paths: &[String], run_status: bool) -> GitReport {
    let mut report = base_report();
    for p in paths {
        let path = Path::new(p);
        if path.is_dir() {
            report.repos.push(inspect(path, run_status));
        }
    }
    finish(&mut report);
    report
}

fn finish(report: &mut GitReport) {
    report.repos.sort_by(|a, b| {
        b.must_back_up
            .cmp(&a.must_back_up)
            .then_with(|| a.path.cmp(&b.path))
    });
    let n = report.repos.iter().filter(|r| r.must_back_up).count();
    if n > 0 {
        report.warnings.push(format!(
            "{n} repo(s) have work that exists only on this machine (no remote, unpushed commits, \
             or uncommitted/untracked files)."
        ));
    }
}

/// Directories that never contain repos worth reporting and cost a lot to walk.
fn is_noise(p: &Path) -> bool {
    p.file_name().is_some_and(|n| {
        matches!(
            n.to_string_lossy().as_ref(),
            "node_modules" | "target" | ".cache" | "Windows" | "$Recycle.Bin" | "System Volume Information"
        )
    })
}

fn base_report() -> GitReport {
    let mut report = GitReport::default();
    let home = dirs::home_dir();

    if let Some(h) = &home {
        let gc = h.join(".gitconfig");
        if gc.exists() {
            report.global_config = Some(gc.display().to_string());
            if let Ok(text) = std::fs::read_to_string(&gc) {
                report.global_credential_helper = parse_config(&text).get("credential.helper").cloned();
            }
        }
        let creds = h.join(".git-credentials");
        if creds.exists() {
            report.git_credentials_file = Some(creds.display().to_string());
            report.warnings.push(
                "~/.git-credentials exists and holds tokens in PLAINTEXT. It will be sealed with \
                 your passphrase rather than copied as-is — and you should rotate those tokens."
                    .into(),
            );
        }
        let ssh = h.join(".ssh");
        if ssh.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&ssh) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name == "config" || name.starts_with("id_") || name.starts_with("known_hosts")
                    {
                        report.ssh_keys.push(e.path().display().to_string());
                    }
                }
            }
            if report.ssh_keys.iter().any(|k| !k.ends_with(".pub")) {
                report.warnings.push(
                    "~/.ssh contains private keys. They are sealed with your passphrase; if any \
                     key has no passphrase of its own, treat the backup as key material."
                        .into(),
                );
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
[core]
	repositoryformatversion = 0
[remote "origin"]
	url = https://github.com/example/repo.git
	fetch = +refs/heads/*:refs/remotes/origin/*
[remote "upstream"]
	url = git@github.com:upstream/repo.git
[credential]
	helper = manager-core
"#;

    #[test]
    fn parses_subsectioned_config_keys() {
        let c = parse_config(CONFIG);
        assert_eq!(
            c.get("remote.origin.url").map(String::as_str),
            Some("https://github.com/example/repo.git")
        );
        assert_eq!(c.get("credential.helper").map(String::as_str), Some("manager-core"));
    }

    #[test]
    fn classifies_remote_url_schemes() {
        assert_eq!(scheme_of("https://github.com/a/b.git"), "https");
        assert_eq!(scheme_of("git@github.com:a/b.git"), "ssh");
        assert_eq!(scheme_of("ssh://git@host/a/b.git"), "ssh");
        assert_eq!(scheme_of("git://host/a/b.git"), "git");
        assert_eq!(scheme_of(r"C:\repos\bare.git"), "file");
    }

    #[test]
    fn last_definition_wins() {
        let c = parse_config("[credential]\nhelper = store\n[credential]\nhelper = wincred\n");
        assert_eq!(c.get("credential.helper").map(String::as_str), Some("wincred"));
    }

    #[test]
    fn skips_expensive_noise_directories() {
        assert!(is_noise(Path::new("/x/node_modules")));
        assert!(!is_noise(Path::new("/x/src")));
    }
}
