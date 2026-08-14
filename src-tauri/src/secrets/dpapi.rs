//! Thin, careful wrapper over the Windows Data Protection API.
//!
//! # Why not the `dpapi-core` crate
//! The spec named `dpapi-core`, but that crate implements **DPAPI-NG**
//! (MS-GKDI group-key blobs used by LAPS/AD) — a different protocol from the
//! per-user `CryptProtectData`/`CryptUnprotectData` pair that Chromium uses.
//! Calling the Win32 API directly is 40 lines, has no supply-chain surface,
//! and is exactly what Chromium itself does.
//!
//! # Threat model reminder
//! DPAPI keys are derived from the user's logon credentials and are only
//! available **inside that user's logon session**. This code therefore:
//! * works for the currently-logged-in user, and
//! * cannot decrypt another account's blobs, or blobs from an offline disk.
//!
//! That's a feature, not a limitation to work around.

#![cfg(windows)]

use crate::error::{AppError, AppResult};
use std::ffi::c_void;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use zeroize::Zeroizing;

/// Never show the DPAPI consent UI — this runs on a background thread.
const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

/// Decrypt a DPAPI blob produced by the current user.
pub fn unprotect(data: &[u8]) -> AppResult<Zeroizing<Vec<u8>>> {
    if data.is_empty() {
        return Ok(Zeroizing::new(Vec::new()));
    }
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();

        CryptUnprotectData(
            &input,
            None, // we don't want the description string back
            None, // no optional entropy — Chromium doesn't use any
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| {
            AppError::Crypto(format!(
                "CryptUnprotectData failed ({e}). This blob likely belongs to a different \
                 Windows account, or the profile was copied from another machine."
            ))
        })?;

        let plain = Zeroizing::new(
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec(),
        );

        // Wipe CryptoAPI's own copy before handing the memory back to the heap.
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        let _ = LocalFree(HLOCAL(output.pbData as *mut c_void));

        Ok(plain)
    }
}

/// Re-protect data for the current user. Used only by the *restore* path, so a
/// restored profile can be re-encrypted in-place on the new install.
pub fn protect(data: &[u8]) -> AppResult<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();

        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| AppError::Crypto(format!("CryptProtectData failed: {e}")))?;

        let out = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut c_void));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_current_logon_session() {
        let secret = b"rebackup dpapi self-test";
        let blob = protect(secret).expect("CryptProtectData");
        assert_ne!(&blob[..], &secret[..], "blob must not be plaintext");
        let back = unprotect(&blob).expect("CryptUnprotectData");
        assert_eq!(&back[..], &secret[..]);
    }
}
