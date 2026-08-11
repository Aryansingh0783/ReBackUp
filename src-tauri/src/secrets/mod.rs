//! Everything that handles secret material.
//!
//! # Invariants enforced across this module
//! 1. A decrypted secret exists only inside a `Zeroizing` container.
//! 2. Nothing secret is ever written to disk unsealed. Sealing happens in
//!    memory and the sealed blob is what touches the filesystem.
//! 3. Nothing secret is ever `tracing::` logged, put in an `AppError`, or
//!    included in the manifest — only counts, lengths and identifiers.
//! 4. Every type carrying a secret has no `Serialize` impl.

#[cfg(windows)]
pub mod chromium;
#[cfg(windows)]
pub mod dpapi;
#[cfg(windows)]
pub mod vault;

use crate::crypto::{self, SealedBlob, SecretString};
use crate::error::AppResult;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Result of a secret-collection pass. Safe to serialise: counts only.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecretsReport {
    pub sealed_files: Vec<SealedArtifact>,
    pub warnings: Vec<String>,
    pub advisories: Vec<String>,
}

#[derive(Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SealedArtifact {
    pub path: String,
    pub label: String,
    pub source: String,
    pub items: usize,
    pub bytes: u64,
    pub sha256: String,
    /// Always true. Present so a reader of the manifest can assert on it.
    pub encrypted: bool,
    pub algorithm: String,
    pub kdf: String,
}

fn seal_to(
    staging: &Path,
    rel: &str,
    plaintext: &[u8],
    pass: &SecretString,
    label: &str,
    source: &str,
    items: usize,
) -> AppResult<SealedArtifact> {
    let blob = crypto::seal(plaintext, pass, label)?;
    let out = staging.join(rel);
    crypto::write_sealed(&out, &blob)?;
    let bytes = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    Ok(SealedArtifact {
        path: out.display().to_string(),
        label: label.to_string(),
        source: source.to_string(),
        items,
        bytes,
        sha256: crypto::sha256_file(&out)?,
        encrypted: true,
        algorithm: "aes-256-gcm".into(),
        kdf: "argon2id".into(),
    })
}

/// Decrypt a Chromium profile's saved logins and seal them as a CSV.
///
/// The plaintext CSV never leaves memory: it is built, sealed, and dropped
/// (zeroized) inside this function.
#[cfg(windows)]
pub fn seal_browser_logins(
    staging: &Path,
    profile_dir: &Path,
    local_state: &Path,
    label: &str,
    pass: &SecretString,
) -> AppResult<(SealedArtifact, chromium::ExtractionReport)> {
    let (records, report) = chromium::extract_logins(profile_dir, local_state)?;
    if records.is_empty() {
        return Err(crate::error::AppError::Other(format!(
            "no decryptable logins in {} ({} row(s) blocked by app-bound encryption)",
            profile_dir.display(),
            report.app_bound_blocked
        )));
    }
    let csv = chromium::to_csv(&records);
    let artifact = seal_to(
        staging,
        &format!("secrets/{}-passwords.csv.prb", slug(label)),
        csv.as_bytes(),
        pass,
        &format!("{label} passwords (Chromium CSV)"),
        &profile_dir.display().to_string(),
        records.len(),
    )?;
    // `records` and `csv` are Zeroizing / hold Zeroizing fields; both wipe here.
    drop(csv);
    drop(records);
    Ok((artifact, report))
}

/// Seal an existing plaintext CSV the user exported by hand, then shred it.
pub fn seal_exported_csv(
    staging: &Path,
    csv_path: &Path,
    label: &str,
    pass: &SecretString,
    shred_source: bool,
) -> AppResult<SealedArtifact> {
    let bytes = zeroize::Zeroizing::new(std::fs::read(csv_path)?);
    let rows = bytes.iter().filter(|b| **b == b'\n').count().saturating_sub(1);
    let artifact = seal_to(
        staging,
        &format!("secrets/{}-passwords.csv.prb", slug(label)),
        &bytes,
        pass,
        &format!("{label} passwords (manual export)"),
        &csv_path.display().to_string(),
        rows,
    )?;
    if shred_source {
        shred(csv_path)?;
    }
    Ok(artifact)
}

/// Seal an arbitrary sensitive file (SSH key, `.git-credentials`, token store).
pub fn seal_file(
    staging: &Path,
    src: &Path,
    rel: &str,
    label: &str,
    pass: &SecretString,
) -> AppResult<SealedArtifact> {
    let bytes = zeroize::Zeroizing::new(std::fs::read(src)?);
    seal_to(
        staging,
        rel,
        &bytes,
        pass,
        label,
        &src.display().to_string(),
        1,
    )
}

/// Overwrite then unlink.
///
/// On an SSD with wear levelling this is *best effort* — the controller may
/// have already relocated the original blocks. The README says so plainly
/// rather than implying a guarantee.
pub fn shred(path: &Path) -> AppResult<()> {
    use std::io::{Seek, SeekFrom, Write};
    let len = std::fs::metadata(path)?.len();
    {
        let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
        let block = vec![0u8; 64 * 1024];
        for _pass in 0..2 {
            f.seek(SeekFrom::Start(0))?;
            let mut left = len;
            while left > 0 {
                let n = left.min(block.len() as u64) as usize;
                f.write_all(&block[..n])?;
                left -= n as u64;
            }
            f.flush()?;
            f.sync_all()?;
        }
    }
    std::fs::remove_file(path)?;
    Ok(())
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .replace("--", "-")
}

/// Read back a sealed artifact. Used by the CLI `unseal` subcommand and by the
/// verify pass.
pub fn open_sealed(path: &PathBuf, pass: &SecretString) -> AppResult<zeroize::Zeroizing<Vec<u8>>> {
    let blob: SealedBlob = crypto::read_sealed(path)?;
    crypto::open(&blob, pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_labels_for_filenames() {
        assert_eq!(slug("Opera GX / Default"), "opera-gx-default");
        assert_eq!(slug("Google Chrome"), "google-chrome");
    }

    #[test]
    fn seals_a_file_and_leaves_no_plaintext_behind() {
        let tmp = std::env::temp_dir().join(format!("prb-seal-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("token.txt");
        std::fs::write(&src, b"ghp_TOTALLY_REAL_TOKEN").unwrap();

        let pass = SecretString::new("a very long all lowercase passphrase".into());
        let art = seal_file(&tmp, &src, "secrets/token.prb", "token", &pass).unwrap();
        assert!(art.encrypted);

        let ondisk = std::fs::read_to_string(&art.path).unwrap();
        assert!(!ondisk.contains("ghp_TOTALLY_REAL_TOKEN"));

        let back = open_sealed(&PathBuf::from(&art.path), &pass).unwrap();
        assert_eq!(&back[..], b"ghp_TOTALLY_REAL_TOKEN");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn shred_removes_the_file() {
        let p = std::env::temp_dir().join(format!("prb-shred-{}.txt", std::process::id()));
        std::fs::write(&p, b"secret").unwrap();
        shred(&p).unwrap();
        assert!(!p.exists());
    }
}
