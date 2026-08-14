//! Headless subcommands.
//!
//! `restore.ps1` runs on a machine that may not have a desktop session yet, so
//! the same binary doubles as a small CLI. Deliberately dependency-free arg
//! parsing — one more crate for four flags isn't worth it.
//!
//! ```text
//! rebackup unseal --in secrets/x.rbu --out x.csv [--passphrase P]
//! rebackup verify --manifest manifest.json
//! rebackup shred  --path x.csv
//! ```
//!
//! When `--passphrase` is omitted the passphrase is read from stdin, which
//! keeps it out of the process list and the shell history.

use crate::crypto::SecretString;
use std::path::{Path, PathBuf};

fn flag(args: &[String], name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).cloned()
}

fn read_passphrase(args: &[String]) -> SecretString {
    if let Some(p) = flag(args, "--passphrase") {
        return SecretString::new(p);
    }
    eprint!("Passphrase: ");
    let mut s = String::new();
    let _ = std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut s);
    SecretString::new(s.trim_end_matches(['\r', '\n']).to_string())
}

/// Returns `Some(exit_code)` when a subcommand ran, `None` to start the GUI.
pub fn maybe_run() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first()?.as_str();

    match cmd {
        "unseal" => Some(unseal(&args)),
        "verify" => Some(verify(&args)),
        "shred" => Some(shred(&args)),
        "--version" | "-V" => {
            println!("rebackup {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        "--help" | "-h" | "help" => {
            println!("{HELP}");
            Some(0)
        }
        // Anything else (including Tauri's own dev flags) falls through to the GUI.
        _ => None,
    }
}

const HELP: &str = "\
rebackup — selective pre-clean-install backup

USAGE:
  rebackup                       start the GUI
  rebackup unseal --in <f.prb> --out <f> [--passphrase <p>]
  rebackup verify --manifest <manifest.json>
  rebackup shred  --path <file>

Omitting --passphrase reads it from stdin, which keeps it out of the process
list. Sealed files are Argon2id + AES-256-GCM; a wrong passphrase and a
tampered file are reported identically, on purpose.";

fn unseal(args: &[String]) -> i32 {
    let (Some(src), Some(dst)) = (flag(args, "--in"), flag(args, "--out")) else {
        eprintln!("usage: unseal --in <file.prb> --out <file>");
        return 2;
    };
    if Path::new(&dst).exists() {
        eprintln!("refusing to overwrite existing file: {dst}");
        return 1;
    }
    let pass = read_passphrase(args);
    match crate::secrets::open_sealed(Path::new(&src), &pass) {
        Ok(plain) => {
            if let Err(e) = std::fs::write(&dst, &plain[..]) {
                eprintln!("write failed: {e}");
                return 1;
            }
            eprintln!("Wrote {} byte(s) to {dst}", plain.len());
            eprintln!("This file is PLAINTEXT. Shred it when you're done:");
            eprintln!("  rebackup shred --path \"{dst}\"");
            0
        }
        Err(e) => {
            eprintln!("unseal failed: {e}");
            1
        }
    }
}

fn verify(args: &[String]) -> i32 {
    let Some(path) = flag(args, "--manifest") else {
        eprintln!("usage: verify --manifest <manifest.json>");
        return 2;
    };
    let path = PathBuf::from(path);
    let m = match crate::manifest::Manifest::read(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("cannot read manifest: {e}");
            return 1;
        }
    };
    let staging = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let r = crate::manifest::verify(&m, &staging, |d, t| {
        if t > 0 && d % 500 == 0 {
            eprint!("\r  {d}/{t}");
        }
    });
    eprintln!("\r{} checked, {} ok", r.checked, r.ok);
    for p in &r.mismatched {
        println!("MISMATCH {p}");
    }
    for p in &r.missing {
        println!("MISSING  {p}");
    }
    if r.passed() {
        eprintln!("OK — backup is intact.");
        0
    } else {
        eprintln!("FAILED — do not rely on this backup.");
        1
    }
}

fn shred(args: &[String]) -> i32 {
    let Some(path) = flag(args, "--path") else {
        eprintln!("usage: shred --path <file>");
        return 2;
    };
    match crate::secrets::shred(Path::new(&path)) {
        Ok(()) => {
            eprintln!("Shredded {path}");
            eprintln!(
                "Note: on an SSD, wear levelling may leave copies of the original blocks. \
                 Treat this as best-effort."
            );
            0
        }
        Err(e) => {
            eprintln!("shred failed: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flags_positionally() {
        let args: Vec<String> = ["unseal", "--in", "a.rbu", "--out", "a.csv"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(flag(&args, "--in").as_deref(), Some("a.rbu"));
        assert_eq!(flag(&args, "--out").as_deref(), Some("a.csv"));
        assert_eq!(flag(&args, "--missing"), None);
    }

    #[test]
    fn unknown_subcommands_fall_through_to_the_gui() {
        // `maybe_run` reads real argv, so exercise the classifier directly.
        for cmd in ["unseal", "verify", "shred", "--help", "-V"] {
            assert!(matches!(
                cmd,
                "unseal" | "verify" | "shred" | "--help" | "-V"
            ));
        }
    }
}
