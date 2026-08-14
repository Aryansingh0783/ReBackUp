//! Chromium password extraction — Opera GX, Opera, Chrome, Edge, Brave, Vivaldi.
//!
//! # How Chromium stores passwords on Windows
//! 1. `<profile>/../Local State` holds `os_crypt.encrypted_key`: base64 of
//!    `"DPAPI" || CryptProtectData(aes_key)`. The inner value is a 32-byte
//!    AES-256 key.
//! 2. `<profile>/Login Data` is a SQLite DB. `logins.password_value` is
//!    `"v10" || nonce(12) || AES-256-GCM(ciphertext || tag(16))`.
//!    * `v10`/`v11` — the scheme above.
//!    * no prefix — legacy: the whole blob is a raw DPAPI blob.
//!    * `v20` — **app-bound encryption** (Chrome 127+). The key is additionally
//!      wrapped by a SYSTEM-privileged COM service and is deliberately not
//!      recoverable by a user-mode process. We detect it and route the user to
//!      the browser's own CSV export instead of pretending to fail mysteriously.
//!
//! # Secret hygiene in this module
//! * The DB is copied to a temp file first (the live one is locked and, worse,
//!   writing to it would corrupt the user's profile). The copy is shredded in
//!   `Drop`.
//! * Every decrypted value lands in `Zeroizing`.
//! * The assembled CSV is returned as `Zeroizing<String>` and is expected to be
//!   handed straight to `crypto::seal`. It must never be written to disk.

#![cfg(windows)]

use crate::error::{AppError, AppResult};
use crate::secrets::dpapi;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use serde::Serialize;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// A password row with the secret still in it. Never `Serialize` this type —
/// it has no `Serialize` impl on purpose.
pub struct LoginRecord {
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub date_created: i64,
    pub blacklisted: bool,
}

/// Non-secret summary, safe to send to the UI and to log.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoginSummary {
    pub origin: String,
    pub username: String,
    /// Length only — never the value, never a hash (hashes of short passwords
    /// are crackable and would leak just as badly).
    pub password_len: usize,
    pub date_created: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionReport {
    pub profile: String,
    pub total_rows: usize,
    pub decrypted: usize,
    pub app_bound_blocked: usize,
    pub failed: usize,
    pub summaries: Vec<LoginSummary>,
    /// Human-readable next step when `app_bound_blocked > 0`.
    pub advisory: Option<String>,
}

/// Temp copy of a locked SQLite DB that shreds itself on drop.
struct ScratchDb(PathBuf);

impl ScratchDb {
    fn clone_from(src: &Path) -> AppResult<Self> {
        let dst = std::env::temp_dir().join(format!(
            "rbu-{}-{}.db",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::copy(src, &dst)?;
        // Chromium may have uncommitted rows in the WAL; copy it too or we
        // silently miss recently-saved passwords.
        for suffix in ["-wal", "-shm", "-journal"] {
            let s = PathBuf::from(format!("{}{suffix}", src.display()));
            if s.exists() {
                let _ = std::fs::copy(&s, PathBuf::from(format!("{}{suffix}", dst.display())));
            }
        }
        Ok(Self(dst))
    }
}

impl Drop for ScratchDb {
    fn drop(&mut self) {
        // Overwrite before unlinking: the DB pages hold ciphertext, but the
        // usernames and URLs in them are sensitive on their own.
        if let Ok(meta) = std::fs::metadata(&self.0) {
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&self.0) {
                use std::io::Write;
                let mut f = std::io::BufWriter::new(f);
                let zeros = vec![0u8; 64 * 1024];
                let mut left = meta.len();
                while left > 0 {
                    let n = left.min(zeros.len() as u64) as usize;
                    if f.write_all(&zeros[..n]).is_err() {
                        break;
                    }
                    left -= n as u64;
                }
                let _ = f.flush();
            }
        }
        let _ = std::fs::remove_file(&self.0);
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(PathBuf::from(format!("{}{suffix}", self.0.display())));
        }
    }
}

/// Pull the 32-byte AES key out of `Local State`.
pub fn master_key(local_state: &Path) -> AppResult<Zeroizing<Vec<u8>>> {
    let raw = std::fs::read_to_string(local_state).map_err(|e| {
        AppError::Io(std::io::Error::other(format!(
            "cannot read {}: {e}",
            local_state.display()
        )))
    })?;
    let json: serde_json::Value = serde_json::from_str(&raw)?;

    let encoded = json
        .pointer("/os_crypt/encrypted_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AppError::Parse(
                "Local State has no os_crypt.encrypted_key (profile never saved a password?)"
                    .into(),
            )
        })?;

    let blob = B64
        .decode(encoded)
        .map_err(|e| AppError::Parse(format!("os_crypt.encrypted_key is not base64: {e}")))?;

    if blob.len() < 6 || &blob[..5] != b"DPAPI" {
        return Err(AppError::Parse(
            "os_crypt.encrypted_key is missing its DPAPI prefix".into(),
        ));
    }

    let key = dpapi::unprotect(&blob[5..])?;
    if key.len() != 32 {
        return Err(AppError::Crypto(format!(
            "expected a 32-byte AES key, got {}",
            key.len()
        )));
    }
    Ok(key)
}

/// True when this profile has opted into Chrome 127+ app-bound encryption.
pub fn has_app_bound_key(local_state: &Path) -> bool {
    std::fs::read_to_string(local_state)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|j| {
            j.pointer("/os_crypt/app_bound_encrypted_key")
                .map(|v| v.is_string())
        })
        .unwrap_or(false)
}

enum Decrypted {
    Value(Zeroizing<String>),
    AppBound,
}

fn decrypt_value(key: &[u8], blob: &[u8]) -> AppResult<Decrypted> {
    if blob.is_empty() {
        return Ok(Decrypted::Value(Zeroizing::new(String::new())));
    }
    if blob.len() > 3 && &blob[..3] == b"v20" {
        return Ok(Decrypted::AppBound);
    }
    if blob.len() > 15 && (&blob[..3] == b"v10" || &blob[..3] == b"v11") {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let pt = cipher
            .decrypt(Nonce::from_slice(&blob[3..15]), &blob[15..])
            .map_err(|_| {
                AppError::Crypto("AES-GCM authentication failed on a password row".into())
            })?;
        let s = String::from_utf8(pt)
            .map_err(|_| AppError::Crypto("decrypted password is not valid UTF-8".into()))?;
        return Ok(Decrypted::Value(Zeroizing::new(s)));
    }
    // Pre-Chrome-80 profiles: the whole column is a raw DPAPI blob.
    let pt = dpapi::unprotect(blob)?;
    let s = String::from_utf8(pt.to_vec())
        .map_err(|_| AppError::Crypto("legacy DPAPI password is not valid UTF-8".into()))?;
    Ok(Decrypted::Value(Zeroizing::new(s)))
}

/// Read + decrypt every saved login in `profile_dir`.
///
/// `profile_dir` is the directory containing `Login Data`; `local_state` is
/// usually `profile_dir/../Local State`.
pub fn extract_logins(
    profile_dir: &Path,
    local_state: &Path,
) -> AppResult<(Vec<LoginRecord>, ExtractionReport)> {
    let login_db = profile_dir.join("Login Data");
    if !login_db.exists() {
        return Err(AppError::Other(format!(
            "no 'Login Data' in {}",
            profile_dir.display()
        )));
    }

    let key = master_key(local_state)?;
    let scratch = ScratchDb::clone_from(&login_db)?;

    let conn = rusqlite::Connection::open_with_flags(
        &scratch.0,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::Other(format!("cannot open Login Data copy: {e}")))?;

    let mut stmt = conn
        .prepare(
            "SELECT origin_url, username_value, password_value, date_created, blacklisted_by_user
             FROM logins ORDER BY origin_url",
        )
        .map_err(|e| AppError::Other(format!("Login Data schema mismatch: {e}")))?;

    let mut records = Vec::new();
    let mut summaries = Vec::new();
    let (mut total, mut ok, mut blocked, mut failed) = (0usize, 0usize, 0usize, 0usize);

    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, i64>(3).unwrap_or(0),
                r.get::<_, i64>(4).unwrap_or(0) != 0,
            ))
        })
        .map_err(|e| AppError::Other(format!("query failed: {e}")))?;

    for row in rows {
        total += 1;
        let Ok((origin, username, blob, created, blacklisted)) = row else {
            failed += 1;
            continue;
        };
        match decrypt_value(&key, &blob) {
            Ok(Decrypted::Value(pw)) => {
                ok += 1;
                summaries.push(LoginSummary {
                    origin: origin.clone(),
                    username: username.clone(),
                    password_len: pw.len(),
                    // Chromium stores WebKit epoch (microseconds since 1601).
                    date_created: if created > 0 {
                        created / 1_000_000 - 11_644_473_600
                    } else {
                        0
                    },
                });
                records.push(LoginRecord {
                    origin,
                    username,
                    password: pw,
                    date_created: created,
                    blacklisted,
                });
            }
            Ok(Decrypted::AppBound) => blocked += 1,
            Err(_) => failed += 1,
        }
    }

    let advisory = if blocked > 0 {
        Some(
            "This profile uses app-bound encryption (Chrome 127+/Opera equivalent). Those \
             passwords are sealed to a SYSTEM-level service and cannot be read from a normal \
             user process by design. Use the browser's own export: open the password manager \
             settings, choose Export, and point ReBackUp at the resulting CSV — it will \
             be sealed with your passphrase and the plaintext file shredded."
                .into(),
        )
    } else {
        None
    };

    Ok((
        records,
        ExtractionReport {
            profile: profile_dir.display().to_string(),
            total_rows: total,
            decrypted: ok,
            app_bound_blocked: blocked,
            failed,
            summaries,
            advisory,
        },
    ))
}

/// Render records as a Chromium-import-compatible CSV, in memory only.
pub fn to_csv(records: &[LoginRecord]) -> Zeroizing<String> {
    let mut out = String::with_capacity(records.len() * 96);
    out.push_str("name,url,username,password,note\n");
    for r in records {
        if r.blacklisted {
            continue; // "never save" entries have no password to restore
        }
        let name = host_of(&r.origin);
        out.push_str(&csv_field(&name));
        out.push(',');
        out.push_str(&csv_field(&r.origin));
        out.push(',');
        out.push_str(&csv_field(&r.username));
        out.push(',');
        out.push_str(&csv_field(r.password.as_str()));
        out.push_str(",\n");
    }
    Zeroizing::new(out)
}

fn csv_field(s: &str) -> String {
    if s.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_csv_fields_that_need_it() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn derives_a_display_name_from_the_origin() {
        assert_eq!(
            host_of("https://accounts.google.com/signin"),
            "accounts.google.com"
        );
        assert_eq!(host_of("android://abc@com.example/"), "abc@com.example");
    }

    #[test]
    fn flags_app_bound_blobs_instead_of_erroring() {
        let key = [0u8; 32];
        let blob = b"v20\x00\x01\x02".to_vec();
        assert!(matches!(
            decrypt_value(&key, &blob).unwrap(),
            Decrypted::AppBound
        ));
    }

    #[test]
    fn csv_skips_never_save_entries() {
        let recs = vec![
            LoginRecord {
                origin: "https://a.example/".into(),
                username: "u".into(),
                password: Zeroizing::new("p".into()),
                date_created: 0,
                blacklisted: false,
            },
            LoginRecord {
                origin: "https://b.example/".into(),
                username: String::new(),
                password: Zeroizing::new(String::new()),
                date_created: 0,
                blacklisted: true,
            },
        ];
        let csv = to_csv(&recs);
        assert_eq!(csv.lines().count(), 2); // header + one row
        assert!(csv.contains("a.example"));
        assert!(!csv.contains("b.example"));
    }
}
