//! Self-contained HTML report written next to the backup.
//!
//! No external CSS, fonts or scripts — it has to open on a freshly installed
//! machine with no network. Everything is escaped; a filename containing
//! `<script>` must not become one.

use crate::error::AppResult;
use crate::manifest::{Manifest, VerifyResult};
use crate::util::human_bytes;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

const CSS: &str = r#"
:root{color-scheme:dark}
*{box-sizing:border-box}
body{margin:0;padding:32px;background:#0b0d10;color:#dfe5ec;
     font:14px/1.55 ui-sans-serif,-apple-system,Segoe UI,Roboto,sans-serif}
h1{font-size:22px;margin:0 0 4px}h2{font-size:15px;margin:32px 0 10px;color:#9fb0c3;
   text-transform:uppercase;letter-spacing:.08em}
.sub{color:#7c8a9a;margin-bottom:24px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:12px}
.card{background:#12151a;border:1px solid #1f2630;border-radius:10px;padding:14px}
.card .k{color:#7c8a9a;font-size:11px;text-transform:uppercase;letter-spacing:.06em}
.card .v{font-size:20px;margin-top:4px;font-variant-numeric:tabular-nums}
table{width:100%;border-collapse:collapse;margin-top:8px;font-size:13px}
th{text-align:left;color:#7c8a9a;font-weight:500;padding:6px 8px;border-bottom:1px solid #1f2630}
td{padding:6px 8px;border-bottom:1px solid #161b22;vertical-align:top}
td.n{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}
code,.mono{font-family:ui-monospace,Consolas,monospace;font-size:12px;color:#a8c7fa}
.pill{display:inline-block;padding:2px 8px;border-radius:999px;font-size:11px;font-weight:600}
.ok{background:#0f2e1f;color:#5ed39a}.bad{background:#331416;color:#ff8b8f}
.warn{background:#332714;color:#f5c15f}
ul{margin:8px 0;padding-left:20px}li{margin:4px 0}
.note{background:#12151a;border-left:3px solid #f5a524;padding:10px 14px;margin:8px 0;border-radius:0 8px 8px 0}
.foot{margin-top:40px;color:#57646f;font-size:12px;border-top:1px solid #1f2630;padding-top:14px}
"#;

pub fn render(manifest: &Manifest, verify: &VerifyResult) -> String {
    let mut by_profile: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    for e in &manifest.entries {
        let slot = by_profile.entry(e.profile.as_str()).or_insert((0, 0));
        slot.0 += 1;
        slot.1 += e.bytes;
    }

    let mut largest: Vec<_> = manifest.entries.iter().collect();
    largest.sort_by_key(|e| std::cmp::Reverse(e.bytes));
    largest.truncate(25);

    let status = if verify.passed() {
        r#"<span class="pill ok">VERIFIED</span>"#
    } else {
        r#"<span class="pill bad">VERIFICATION FAILED</span>"#
    };

    let mut h = String::with_capacity(64 * 1024);
    h.push_str(&format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Pre-Reset Backup — {}</title><style>{CSS}</style></head><body>",
        esc(&manifest.machine)
    ));

    h.push_str(&format!(
        "<h1>Pre-Reset Backup {status}</h1>\
         <div class=\"sub\">{} &middot; {}\\{} &middot; {}</div>",
        esc(&manifest.created),
        esc(&manifest.machine),
        esc(&manifest.user),
        esc(manifest.windows_build.as_deref().unwrap_or("unknown Windows build"))
    ));

    // --- headline numbers ---------------------------------------------------
    let card = |k: &str, v: String| format!("<div class=\"card\"><div class=\"k\">{k}</div><div class=\"v\">{v}</div></div>");
    h.push_str("<div class=\"grid\">");
    h.push_str(&card("Files", manifest.file_count.to_string()));
    h.push_str(&card("Size", human_bytes(manifest.total_bytes)));
    h.push_str(&card("Sealed artifacts", manifest.sealed.len().to_string()));
    h.push_str(&card(
        "Hashes checked",
        format!("{} / {}", verify.ok, verify.checked),
    ));
    if let Some(a) = &manifest.archive {
        h.push_str(&card("Archive", human_bytes(a.bytes)));
    }
    h.push_str(&card("Skipped", manifest.skipped.len().to_string()));
    h.push_str("</div>");

    // --- verification -------------------------------------------------------
    if !verify.passed() {
        h.push_str("<h2>Verification failures</h2><div class=\"note\">");
        h.push_str("These files did not match their recorded hash. <b>Do not reset until this is resolved.</b><ul>");
        for p in verify.mismatched.iter().chain(verify.missing.iter()).take(50) {
            h.push_str(&format!("<li class=\"mono\">{}</li>", esc(p)));
        }
        h.push_str("</ul></div>");
    }

    // --- by profile ---------------------------------------------------------
    h.push_str("<h2>What was backed up</h2><table><tr><th>Profile</th><th class=\"n\">Files</th><th class=\"n\">Size</th></tr>");
    for (p, (n, b)) in &by_profile {
        h.push_str(&format!(
            "<tr><td>{}</td><td class=\"n\">{n}</td><td class=\"n\">{}</td></tr>",
            esc(p),
            human_bytes(*b)
        ));
    }
    h.push_str("</table>");

    // --- sealed -------------------------------------------------------------
    if !manifest.sealed.is_empty() {
        h.push_str(
            "<h2>Encrypted artifacts</h2>\
             <p class=\"sub\">Argon2id-derived key, AES-256-GCM. Plaintext never touched the disk. \
             Open with <code>pre-reset-backup.exe unseal</code>.</p>\
             <table><tr><th>File</th><th>Contents</th><th class=\"n\">Items</th><th class=\"n\">Size</th></tr>",
        );
        for s in &manifest.sealed {
            let name = Path::new(&s.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.path.clone());
            h.push_str(&format!(
                "<tr><td class=\"mono\">{}</td><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
                esc(&name),
                esc(&s.label),
                s.items,
                human_bytes(s.bytes)
            ));
        }
        h.push_str("</table>");
    }

    // --- largest files ------------------------------------------------------
    if !largest.is_empty() {
        h.push_str("<h2>Largest files</h2><table><tr><th>Path</th><th class=\"n\">Size</th></tr>");
        for e in largest {
            h.push_str(&format!(
                "<tr><td class=\"mono\">{}</td><td class=\"n\">{}</td></tr>",
                esc(&e.source),
                human_bytes(e.bytes)
            ));
        }
        h.push_str("</table>");
    }

    // --- context ------------------------------------------------------------
    h.push_str(&render_context(manifest));

    // --- warnings -----------------------------------------------------------
    if !manifest.warnings.is_empty() {
        h.push_str("<h2>Read before you reset</h2>");
        for w in &manifest.warnings {
            h.push_str(&format!("<div class=\"note\">{}</div>", esc(w)));
        }
    }

    if !manifest.skipped.is_empty() {
        h.push_str("<h2>Skipped</h2><table><tr><th>Path</th><th>Reason</th></tr>");
        for s in manifest.skipped.iter().take(200) {
            h.push_str(&format!(
                "<tr><td class=\"mono\">{}</td><td>{}</td></tr>",
                esc(&s.path),
                esc(&s.reason)
            ));
        }
        h.push_str("</table>");
    }

    h.push_str(
        "<h2>Restore</h2><ol>\
         <li>Copy this entire folder to an external drive <b>before</b> resetting.</li>\
         <li>On the fresh install, run <code>restore.cmd</code> (or <code>restore.ps1 -DryRun</code> first).</li>\
         <li>Unseal the encrypted artifacts and import the password CSV.</li>\
         <li>Shred the CSV once the import succeeds.</li>\
         </ol>",
    );

    h.push_str(&format!(
        "<div class=\"foot\">pre-reset-backup {} &middot; manifest v{} &middot; staging <span class=\"mono\">{}</span></div>",
        esc(&manifest.tool_version),
        manifest.version,
        esc(&manifest.staging_root)
    ));
    h.push_str("</body></html>");
    h
}

/// Steam accounts / git repos / credential inventory, if the run collected them.
fn render_context(manifest: &Manifest) -> String {
    let mut h = String::new();
    let ctx = &manifest.context;

    if let Some(accounts) = ctx.pointer("/steam/accounts").and_then(|v| v.as_array()) {
        if !accounts.is_empty() {
            h.push_str("<h2>Steam accounts</h2><table><tr><th>Account</th><th>Persona</th><th>SteamID64</th><th>Remembered</th></tr>");
            for a in accounts {
                h.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td></tr>",
                    esc(a["accountName"].as_str().unwrap_or("")),
                    esc(a["personaName"].as_str().unwrap_or("")),
                    esc(a["steamId64"].as_str().unwrap_or("")),
                    if a["rememberPassword"].as_bool().unwrap_or(false) { "yes" } else { "no" }
                ));
            }
            h.push_str("</table>");
        }
    }

    if let Some(repos) = ctx.pointer("/git/repos").and_then(|v| v.as_array()) {
        let must: Vec<_> = repos
            .iter()
            .filter(|r| r["mustBackUp"].as_bool().unwrap_or(false))
            .collect();
        if !must.is_empty() {
            h.push_str("<h2>Git repos that exist only here</h2><table><tr><th>Path</th><th>Branch</th><th>Remote</th><th>State</th></tr>");
            for r in must {
                let remote = r["remotes"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|x| x["url"].as_str())
                    .unwrap_or("(none)");
                let mut state = Vec::new();
                if r["dirty"].as_bool().unwrap_or(false) {
                    state.push("uncommitted");
                }
                if r["ahead"].as_u64().unwrap_or(0) > 0 {
                    state.push("unpushed");
                }
                if r["untracked"].as_u64().unwrap_or(0) > 0 {
                    state.push("untracked");
                }
                h.push_str(&format!(
                    "<tr><td class=\"mono\">{}</td><td>{}</td><td class=\"mono\">{}</td><td>{}</td></tr>",
                    esc(r["path"].as_str().unwrap_or("")),
                    esc(r["branch"].as_str().unwrap_or("-")),
                    esc(remote),
                    esc(&if state.is_empty() { "no remote".to_string() } else { state.join(", ") })
                ));
            }
            h.push_str("</table>");
        }
    }

    if let Some(creds) = ctx.get("credentials").and_then(|v| v.as_array()) {
        if !creds.is_empty() {
            h.push_str(&format!(
                "<h2>Windows credentials ({} entries)</h2>\
                 <p class=\"sub\">Inventory only — the secrets themselves are not exported here. \
                 Use the guided .crd export, or plan to re-enter these.</p>\
                 <table><tr><th>Target</th><th>User</th><th>Type</th></tr>",
                creds.len()
            ));
            for c in creds.iter().take(120) {
                h.push_str(&format!(
                    "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td></tr>",
                    esc(c["target"].as_str().unwrap_or("")),
                    esc(c["username"].as_str().unwrap_or("")),
                    esc(c["kind"].as_str().unwrap_or(""))
                ));
            }
            h.push_str("</table>");
        }
    }

    h
}

pub fn write_html(staging: &Path, manifest: &Manifest, verify: &VerifyResult) -> AppResult<PathBuf> {
    let path = staging.join("report.html");
    std::fs::write(&path, render(manifest, verify))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestEntry;

    fn fixture() -> (Manifest, VerifyResult) {
        let mut m = Manifest::new(Path::new("/tmp/prb"), vec!["desktop".into()]);
        m.push(ManifestEntry {
            source: r"C:\Users\a\Desktop\<script>alert(1)</script>.txt".into(),
            staged: "files/C/Users/a/Desktop/x.txt".into(),
            bytes: 10,
            sha256: "deadbeef".into(),
            modified: 0,
            profile: "desktop".into(),
        });
        let v = VerifyResult {
            checked: 1,
            ok: 1,
            mismatched: vec![],
            missing: vec![],
            archive_ok: None,
        };
        (m, v)
    }

    #[test]
    fn escapes_hostile_filenames() {
        let (m, v) = fixture();
        let html = render(&m, &v);
        assert!(!html.contains("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn is_fully_self_contained() {
        let (m, v) = fixture();
        let html = render(&m, &v);
        for remote in ["http://", "https://", "<script", "cdn."] {
            assert!(!html.contains(remote), "report must not reference {remote}");
        }
    }

    #[test]
    fn shows_a_failure_banner_when_verification_fails() {
        let (m, mut v) = fixture();
        v.mismatched.push("files/x".into());
        let html = render(&m, &v);
        assert!(html.contains("VERIFICATION FAILED"));
        assert!(html.contains("Do not reset"));
    }
}
