//! The manifest: the single source of truth for what a backup contains and how
//! to put it back.
//!
//! # Rules
//! * Every staged file gets a SHA-256 recorded **at copy time**, and the verify
//!   pass re-hashes from disk. A mismatch fails the backup loudly.
//! * Secret artifacts appear as `sealed[]` entries with `encrypted: true` and
//!   the algorithm/KDF used. Their *plaintext* never appears, and neither do
//!   password values, tokens, or key material of any kind.
//! * The manifest is written last so its presence means "the backup finished".

use crate::secrets::SealedArtifact;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_NAME: &str = "manifest.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// Absolute path on the source machine.
    pub source: String,
    /// Path relative to the staging root.
    pub staged: String,
    pub bytes: u64,
    pub sha256: String,
    pub modified: i64,
    pub profile: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInfo {
    pub path: String,
    pub format: String,
    pub bytes: u64,
    pub sha256: String,
    pub encrypted: bool,
    pub cipher: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub tool: String,
    pub tool_version: String,
    pub created: String,
    pub machine: String,
    pub user: String,
    pub windows_build: Option<String>,
    pub staging_root: String,
    pub profiles: Vec<String>,
    pub entries: Vec<ManifestEntry>,
    /// Encrypted artifacts. Never contains plaintext.
    pub sealed: Vec<SealedArtifact>,
    pub archive: Option<ArchiveInfo>,
    pub total_bytes: u64,
    pub file_count: usize,
    pub skipped: Vec<SkippedItem>,
    pub warnings: Vec<String>,
    /// Free-form, profile-specific findings (Steam accounts, git repos, ...).
    pub context: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

impl Manifest {
    pub fn new(staging_root: &Path, profiles: Vec<String>) -> Self {
        Self {
            version: MANIFEST_VERSION,
            tool: "pre-reset-backup".into(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
            created: crate::util::rfc3339_now(),
            machine: hostname(),
            user: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_default(),
            windows_build: os_build(),
            staging_root: staging_root.display().to_string(),
            profiles,
            entries: Vec::new(),
            sealed: Vec::new(),
            archive: None,
            total_bytes: 0,
            file_count: 0,
            skipped: Vec::new(),
            warnings: Vec::new(),
            context: serde_json::Value::Null,
        }
    }

    pub fn push(&mut self, e: ManifestEntry) {
        self.total_bytes += e.bytes;
        self.file_count += 1;
        self.entries.push(e);
    }

    pub fn write(&self, dir: &Path) -> crate::error::AppResult<std::path::PathBuf> {
        let path = dir.join(MANIFEST_NAME);
        std::fs::write(&path, serde_json::to_vec_pretty(self)?)?;
        Ok(path)
    }

    pub fn read(path: &Path) -> crate::error::AppResult<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    /// Cheap self-check before the expensive hash pass.
    pub fn audit_for_plaintext_secrets(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for s in &self.sealed {
            if !s.encrypted {
                problems.push(format!("sealed artifact {} is not marked encrypted", s.path));
            }
        }
        // A staged file called *.csv under secrets/ means the shred step failed.
        for e in &self.entries {
            let staged = e.staged.to_lowercase();
            if staged.contains("secrets/") && !staged.ends_with(".prb") {
                problems.push(format!(
                    "unsealed file inside secrets/: {} — refusing to publish",
                    e.staged
                ));
            }
        }
        problems
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub checked: usize,
    pub ok: usize,
    pub mismatched: Vec<String>,
    pub missing: Vec<String>,
    pub archive_ok: Option<bool>,
}

impl VerifyResult {
    pub fn passed(&self) -> bool {
        self.mismatched.is_empty() && self.missing.is_empty() && self.archive_ok != Some(false)
    }
}

/// Re-hash every staged file and compare against the manifest.
pub fn verify(manifest: &Manifest, staging: &Path, mut on_progress: impl FnMut(usize, usize)) -> VerifyResult {
    let mut result = VerifyResult {
        checked: 0,
        ok: 0,
        mismatched: Vec::new(),
        missing: Vec::new(),
        archive_ok: None,
    };
    let total = manifest.entries.len();

    for (i, e) in manifest.entries.iter().enumerate() {
        let p = staging.join(&e.staged);
        result.checked += 1;
        if !p.is_file() {
            result.missing.push(e.staged.clone());
        } else {
            match crate::crypto::sha256_file(&p) {
                Ok(h) if h == e.sha256 => result.ok += 1,
                Ok(_) => result.mismatched.push(e.staged.clone()),
                Err(_) => result.missing.push(e.staged.clone()),
            }
        }
        if i % 64 == 0 {
            on_progress(i, total);
        }
    }

    for s in &manifest.sealed {
        let p = Path::new(&s.path);
        result.checked += 1;
        match crate::crypto::sha256_file(p) {
            Ok(h) if h == s.sha256 => result.ok += 1,
            Ok(_) => result.mismatched.push(s.path.clone()),
            Err(_) => result.missing.push(s.path.clone()),
        }
    }

    if let Some(a) = &manifest.archive {
        result.archive_ok = crate::crypto::sha256_file(Path::new(&a.path))
            .ok()
            .map(|h| h == a.sha256);
    }

    on_progress(total, total);
    result
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

fn os_build() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        use winreg::RegKey;
        let k = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .ok()?;
        let product: String = k.get_value("ProductName").ok()?;
        let display: String = k.get_value("DisplayVersion").unwrap_or_default();
        let build: String = k.get_value("CurrentBuild").unwrap_or_default();
        return Some(format!("{product} {display} (build {build})"));
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(staged: &str) -> ManifestEntry {
        ManifestEntry {
            source: format!(r"C:\{staged}"),
            staged: staged.into(),
            bytes: 3,
            sha256: crate::crypto::sha256_bytes(b"abc"),
            modified: 0,
            profile: "test".into(),
        }
    }

    #[test]
    fn verify_detects_tampering_and_deletion() {
        let dir = std::env::temp_dir().join(format!("prb-man-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join("files/good.txt"), b"abc").unwrap();
        std::fs::write(dir.join("files/bad.txt"), b"xyz").unwrap();

        let mut m = Manifest::new(&dir, vec!["test".into()]);
        m.push(entry("files/good.txt"));
        m.push(entry("files/bad.txt"));
        m.push(entry("files/gone.txt"));

        let r = verify(&m, &dir, |_, _| {});
        assert_eq!(r.ok, 1);
        assert_eq!(r.mismatched, vec!["files/bad.txt"]);
        assert_eq!(r.missing, vec!["files/gone.txt"]);
        assert!(!r.passed());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn audit_rejects_unsealed_files_under_secrets() {
        let mut m = Manifest::new(Path::new("/tmp"), vec![]);
        m.push(entry("secrets/passwords.csv"));
        assert!(!m.audit_for_plaintext_secrets().is_empty());

        let mut ok = Manifest::new(Path::new("/tmp"), vec![]);
        ok.push(entry("secrets/passwords.csv.prb"));
        assert!(ok.audit_for_plaintext_secrets().is_empty());
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let dir = std::env::temp_dir().join(format!("prb-man2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Manifest::new(&dir, vec!["desktop".into()]);
        m.push(entry("files/a.txt"));
        let p = m.write(&dir).unwrap();
        let back = Manifest::read(&p).unwrap();
        assert_eq!(back.file_count, 1);
        assert_eq!(back.version, MANIFEST_VERSION);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
