//! Passphrase-derived envelope encryption for everything secret this app
//! touches.
//!
//! # Design
//! * **KDF**: Argon2id, m=64 MiB, t=3, p=1 (OWASP 2024 baseline). Salt is 16
//!   random bytes, stored in the clear next to the ciphertext.
//! * **AEAD**: AES-256-GCM, 96-bit random nonce, 128-bit tag.
//! * **AAD**: the serialised KDF parameters are fed in as associated data, so
//!   an attacker cannot downgrade `m_cost` on a stored blob and have it still
//!   authenticate.
//! * **Zeroization**: derived keys and plaintexts live in `Zeroizing`
//!   containers and are wiped on drop. `SecretString` never implements
//!   `Display`/`Debug` in a way that leaks.
//!
//! # What this is NOT
//! This is not a streaming format. Blobs are held whole in memory, which is
//! correct for the things we seal (password CSVs, token files, small config)
//! and wrong for multi-gigabyte archives — those go through 7-Zip's own
//! AES-256 instead (see `backup::archive`).

use crate::error::{AppError, AppResult};
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

pub const MAGIC: &str = "PRB1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Passphrase wrapper. Wiped on drop; deliberately has no `Debug` impl that
/// prints the contents.
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(Zeroizing::new(s))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Rough strength floor. We refuse to seal with anything trivially
    /// brute-forceable, because the blob will sit on a USB stick for weeks.
    pub fn check_strength(&self) -> AppResult<()> {
        let s = self.expose();
        if s.chars().count() < 12 {
            return Err(AppError::Crypto(
                "passphrase must be at least 12 characters".into(),
            ));
        }
        let classes = [
            s.chars().any(|c| c.is_lowercase()),
            s.chars().any(|c| c.is_uppercase()),
            s.chars().any(|c| c.is_ascii_digit()),
            s.chars().any(|c| !c.is_alphanumeric()),
        ]
        .iter()
        .filter(|b| **b)
        .count();
        if classes < 3 && s.chars().count() < 20 {
            return Err(AppError::Crypto(
                "passphrase needs 3 of {lowercase, uppercase, digit, symbol}, or 20+ characters"
                    .into(),
            ));
        }
        Ok(())
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(<redacted>)")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub algorithm: String, // "argon2id"
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            algorithm: "argon2id".into(),
            m_cost_kib: 64 * 1024, // 64 MiB
            t_cost: 3,
            p_cost: 1,
        }
    }
}

/// Self-describing sealed blob. Safe to write to disk and to quote in logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedBlob {
    pub magic: String,
    pub cipher: String, // "aes-256-gcm"
    pub kdf: KdfParams,
    pub salt: String,       // base64
    pub nonce: String,      // base64
    pub ciphertext: String, // base64, includes the 16-byte GCM tag
    /// Free-form label so a human can tell blobs apart without decrypting.
    pub label: String,
}

fn derive_key(
    pass: &SecretString,
    salt: &[u8],
    kdf: &KdfParams,
) -> AppResult<Zeroizing<[u8; KEY_LEN]>> {
    let params = Params::new(kdf.m_cost_kib, kdf.t_cost, kdf.p_cost, Some(KEY_LEN))
        .map_err(|e| AppError::Crypto(format!("bad Argon2 params: {e}")))?;
    let a2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    a2.hash_password_into(pass.expose().as_bytes(), salt, key.as_mut())
        .map_err(|e| AppError::Crypto(format!("Argon2id failed: {e}")))?;
    Ok(key)
}

/// Bind the KDF parameters to the ciphertext so they can't be tampered with.
fn aad(kdf: &KdfParams, label: &str) -> Vec<u8> {
    format!(
        "{MAGIC}|{}|{}|{}|{}|{label}",
        kdf.algorithm, kdf.m_cost_kib, kdf.t_cost, kdf.p_cost
    )
    .into_bytes()
}

pub fn seal(plaintext: &[u8], pass: &SecretString, label: &str) -> AppResult<SealedBlob> {
    let kdf = KdfParams::default();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(pass, &salt, &kdf)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: &aad(&kdf, label),
            },
        )
        .map_err(|_| AppError::Crypto("AES-256-GCM seal failed".into()))?;

    Ok(SealedBlob {
        magic: MAGIC.into(),
        cipher: "aes-256-gcm".into(),
        kdf,
        salt: B64.encode(salt),
        nonce: B64.encode(nonce_bytes),
        ciphertext: B64.encode(ct),
        label: label.to_string(),
    })
}

pub fn open(blob: &SealedBlob, pass: &SecretString) -> AppResult<Zeroizing<Vec<u8>>> {
    if blob.magic != MAGIC {
        return Err(AppError::Crypto(format!(
            "unrecognised container magic {:?}",
            blob.magic
        )));
    }
    if blob.cipher != "aes-256-gcm" {
        return Err(AppError::Crypto(format!(
            "unsupported cipher {}",
            blob.cipher
        )));
    }
    let salt = B64
        .decode(&blob.salt)
        .map_err(|e| AppError::Crypto(format!("bad salt: {e}")))?;
    let nonce = B64
        .decode(&blob.nonce)
        .map_err(|e| AppError::Crypto(format!("bad nonce: {e}")))?;
    let mut ct = B64
        .decode(&blob.ciphertext)
        .map_err(|e| AppError::Crypto(format!("bad ciphertext: {e}")))?;
    if nonce.len() != NONCE_LEN {
        return Err(AppError::Crypto("nonce must be 12 bytes".into()));
    }

    let key = derive_key(pass, &salt, &blob.kdf)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let pt = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ct,
                aad: &aad(&blob.kdf, &blob.label),
            },
        )
        // A GCM tag failure is indistinguishable from a wrong passphrase, and
        // that ambiguity is intentional — don't leak which one it was.
        .map_err(|_| {
            AppError::Crypto(
                "decryption failed: wrong passphrase or the blob was tampered with".into(),
            )
        })?;
    ct.zeroize();
    Ok(Zeroizing::new(pt))
}

pub fn write_sealed(path: &std::path::Path, blob: &SealedBlob) -> AppResult<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(blob)?)?;
    Ok(())
}

pub fn read_sealed(path: &std::path::Path) -> AppResult<SealedBlob> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

/// SHA-256 of a file, streamed so a 20 GB VM image doesn't blow up the heap.
pub fn sha256_file(path: &std::path::Path) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256_bytes(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass() -> SecretString {
        SecretString::new("correct horse Battery 9!".into())
    }

    #[test]
    fn round_trips() {
        let blob = seal(b"hunter2\nswordfish", &pass(), "test").unwrap();
        let out = open(&blob, &pass()).unwrap();
        assert_eq!(&out[..], b"hunter2\nswordfish");
    }

    #[test]
    fn rejects_wrong_passphrase() {
        let blob = seal(b"secret", &pass(), "test").unwrap();
        let wrong = SecretString::new("correct horse Battery 8!".into());
        assert!(open(&blob, &wrong).is_err());
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let mut blob = seal(b"secret", &pass(), "test").unwrap();
        let mut raw = B64.decode(&blob.ciphertext).unwrap();
        raw[0] ^= 0xFF;
        blob.ciphertext = B64.encode(raw);
        assert!(open(&blob, &pass()).is_err());
    }

    #[test]
    fn rejects_downgraded_kdf_params() {
        let mut blob = seal(b"secret", &pass(), "test").unwrap();
        blob.kdf.m_cost_kib = 8; // attacker tries to make cracking cheap
        assert!(open(&blob, &pass()).is_err());
    }

    #[test]
    fn plaintext_never_appears_in_the_serialised_blob() {
        let blob = seal(b"SUPER_SECRET_TOKEN", &pass(), "test").unwrap();
        let json = serde_json::to_string(&blob).unwrap();
        assert!(!json.contains("SUPER_SECRET_TOKEN"));
    }

    #[test]
    fn enforces_a_minimum_passphrase() {
        assert!(SecretString::new("short".into()).check_strength().is_err());
        assert!(SecretString::new("alllowercaseletters".into())
            .check_strength()
            .is_err());
        assert!(pass().check_strength().is_ok());
        assert!(
            SecretString::new("a very long all lowercase passphrase".into())
                .check_strength()
                .is_ok()
        );
    }
}
