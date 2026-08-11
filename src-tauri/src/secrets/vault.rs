//! Windows Credential Manager.
//!
//! # What can and cannot be automated
//! Credential Manager's supported export path is the **Stored User Names and
//! Passwords** wizard (`rundll32 keymgr.dll,KRShowKeyMgr` -> "Back up
//! Credentials"), which writes a `.crd` file. That wizard *requires* the secure
//! desktop: Windows switches to the Ctrl+Alt+Del screen to take the protection
//! password, specifically so that no automation — including this app — can type
//! it or read it. There is no supported API to drive it.
//!
//! So we split the job:
//! * **Automatic**: enumerate credential *metadata* (target, user, type,
//!   last-written) via `CredEnumerateW`, so the user gets an inventory of what
//!   they'll need to re-enter and can tell whether the `.crd` covered it.
//! * **Guided**: launch the wizard, watch the chosen output directory for a new
//!   `.crd`, and pull it into staging once it appears.
//!
//! We deliberately do **not** read `CredentialBlob` and dump it to disk. Those
//! blobs are DPAPI-protected per-user; re-exporting them wholesale creates a
//! plaintext-equivalent artifact with none of the `.crd` file's own protection.

#![cfg(windows)]

use crate::error::{AppError, AppResult};
use crate::util::filetime_to_unix;
use serde::Serialize;
use std::path::{Path, PathBuf};
use windows::Win32::Security::Credentials::{
    CredEnumerateW, CredFree, CREDENTIALW, CRED_ENUMERATE_ALL_CREDENTIALS,
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CredentialInfo {
    pub target: String,
    pub username: String,
    pub kind: &'static str,
    pub persist: &'static str,
    pub last_written: i64,
    /// Size of the secret in bytes. The secret itself is never read.
    pub blob_bytes: u32,
}

fn wide_to_string(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string().unwrap_or_default() }
}

/// Inventory every credential visible to the current user.
pub fn enumerate() -> AppResult<Vec<CredentialInfo>> {
    let mut count = 0u32;
    let mut ptr: *mut *mut CREDENTIALW = std::ptr::null_mut();

    unsafe {
        CredEnumerateW(
            windows::core::PCWSTR::null(),
            CRED_ENUMERATE_ALL_CREDENTIALS,
            &mut count,
            &mut ptr,
        )
        .map_err(|e| {
            AppError::Other(format!(
                "CredEnumerateW failed ({e}). Enumerating all credentials requires the process \
                 to be running as the interactive user."
            ))
        })?;

        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let entry = *ptr.add(i);
            if entry.is_null() {
                continue;
            }
            let c = &*entry;
            out.push(CredentialInfo {
                target: wide_to_string(c.TargetName),
                username: wide_to_string(c.UserName),
                kind: match c.Type.0 {
                    1 => "generic",
                    2 => "domain-password",
                    3 => "domain-certificate",
                    4 => "domain-visible-password",
                    5 => "generic-certificate",
                    6 => "domain-extended",
                    _ => "unknown",
                },
                persist: match c.Persist.0 {
                    1 => "session",
                    2 => "local-machine",
                    3 => "enterprise",
                    _ => "unknown",
                },
                last_written: filetime_to_unix(
                    ((c.LastWritten.dwHighDateTime as u64) << 32)
                        | c.LastWritten.dwLowDateTime as u64,
                ),
                // Deliberately NOT reading CredentialBlob — see the module docs.
                blob_bytes: c.CredentialBlobSize,
            });
        }

        CredFree(ptr as *const std::ffi::c_void);
        Ok(out)
    }
}

/// Launch the credential backup wizard. Returns immediately — the wizard runs
/// on the secure desktop and we cannot observe it.
pub fn open_backup_wizard() -> AppResult<()> {
    std::process::Command::new("rundll32.exe")
        .args(["keymgr.dll,KRShowKeyMgr"])
        .spawn()
        .map_err(|e| AppError::Other(format!("could not launch keymgr: {e}")))?;
    Ok(())
}

/// Step-by-step text shown next to the wizard button in the UI. Kept in Rust so
/// the docs, the report and the UI can't drift apart.
pub const WIZARD_STEPS: &[&str] = &[
    "Click 'Back up Credentials' in the window that just opened.",
    "Choose a path INSIDE the staging folder shown above, e.g. staging\\credentials\\vault.crd.",
    "Press Ctrl+Alt+Delete when Windows asks — this switches to the secure desktop.",
    "Type a protection password. Use the SAME passphrase you gave this app, or store it somewhere you will still have after the reset.",
    "Return here and click 'I've finished' so the .crd is hashed into the manifest.",
];

/// Find a `.crd` written under `dir` in the last `within_secs` seconds.
pub fn find_recent_crd(dir: &Path, within_secs: u64) -> Option<PathBuf> {
    let now = std::time::SystemTime::now();
    walkdir::WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("crd"))
        })
        .find(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|d| d.as_secs() <= within_secs)
        })
        .map(|e| e.into_path())
}

/// `vaultcmd /list` output, purely informational. Some Windows editions ship
/// without vaultcmd, so a failure here is not fatal.
pub fn vaultcmd_list() -> Option<String> {
    std::process::Command::new("vaultcmd")
        .arg("/list")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}
