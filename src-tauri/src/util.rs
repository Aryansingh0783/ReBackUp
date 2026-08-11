//! Small helpers shared across modules. Nothing here touches secrets.

use std::path::{Path, PathBuf};

/// Windows FILETIME (100 ns ticks since 1601-01-01) -> Unix seconds.
/// Returns 0 for the null/invalid timestamps that litter the MFT.
pub fn filetime_to_unix(ft: u64) -> i64 {
    if ft == 0 {
        return 0;
    }
    (ft / 10_000_000) as i64 - 11_644_473_600
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `20260811-174233` — sortable, filesystem-safe, no colons.
pub fn timestamp_slug() -> String {
    let fmt = time::macros::format_description!("[year][month][day]-[hour][minute][second]");
    time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(fmt)
        .unwrap_or_else(|_| "unknown".into())
}

pub fn rfc3339_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Turn `C:\Users\a\Desktop\x.txt` into `C/Users/a/Desktop/x.txt` so it can be
/// re-rooted under the staging directory without colliding across volumes.
pub fn stage_relative(src: &Path) -> PathBuf {
    let s = src.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches("//?/");
    let mut out = PathBuf::new();
    for (i, comp) in s.split('/').filter(|c| !c.is_empty()).enumerate() {
        if i == 0 && comp.len() == 2 && comp.ends_with(':') {
            out.push(&comp[..1]); // "C:" -> "C"
        } else {
            out.push(sanitize_component(comp));
        }
    }
    out
}

/// Strip characters that are illegal on NTFS/exFAT so a staged copy of a Linux
/// or network path can still be written to a FAT-formatted USB stick.
pub fn sanitize_component(c: &str) -> String {
    c.chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            ch if (ch as u32) < 0x20 => '_',
            ch => ch,
        })
        .collect()
}

pub fn human_bytes(n: u64) -> String {
    const U: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.2} {}", U[i])
    }
}

/// Expand `%VAR%` (Windows) and `$VAR` / `~` (POSIX) inside a profile target.
pub fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            '%' => {
                if let Some(end) = bytes[i + 1..].iter().position(|c| *c == '%') {
                    let name: String = bytes[i + 1..i + 1 + end].iter().collect();
                    out.push_str(&std::env::var(&name).unwrap_or_default());
                    i += end + 2;
                    continue;
                }
                out.push('%');
            }
            '~' if i == 0 => {
                if let Some(h) = dirs::home_dir() {
                    out.push_str(&h.to_string_lossy());
                } else {
                    out.push('~');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}
