//! The backup engine: plan -> stage -> seal -> archive -> verify -> manifest.
//!
//! # Ordering matters
//! 1. **Plan** resolves globs while nothing is being written, so the size
//!    estimate the user approves is the size they get.
//! 2. **Stage** copies files into `%TEMP%\ReBackUp_<ts>\files\...`,
//!    hashing as it copies (one read, not two).
//! 3. **Seal** runs the secret actions. This happens *after* staging so a
//!    failure in a browser profile can't strand half-copied files.
//! 4. **Archive** compresses staging. With the `sevenz` feature this is
//!    7z/LZMA2 + AES-256; otherwise zip+zstd, unencrypted (and the manifest
//!    says so).
//! 5. **Verify** re-hashes everything from disk. A backup that doesn't verify
//!    is reported as failed — silence here is how people lose data.
//! 6. **Manifest** is written last. Its presence means the run completed.

use crate::crypto::SecretString;
use crate::error::{AppError, AppResult};
use crate::manifest::{ArchiveInfo, Manifest, ManifestEntry, SkippedItem, VerifyResult};
use crate::profiles::{Profile, SecretAction, GLOBAL_EXCLUDES};
use crate::util::{stage_relative, timestamp_slug};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub const EVT_PROGRESS: &str = "backup://progress";
pub const EVT_LOG: &str = "backup://log";
pub const EVT_DONE: &str = "backup://done";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgress {
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub level: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveMode {
    /// Leave the staging folder as-is (fastest; copy it to a USB stick yourself).
    None,
    /// zip + zstd. Fast, portable, NOT encrypted.
    Zip,
    /// 7z/LZMA2 + AES-256. Slowest, smallest, encrypted with your passphrase.
    SevenZip,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSelection {
    pub profile_ids: Vec<String>,
    /// Extra absolute paths hand-picked in the scanner.
    #[serde(default)]
    pub extra_paths: Vec<String>,
    /// Paths to drop from the plan (absolute, matched by prefix).
    #[serde(default)]
    pub excluded_paths: Vec<String>,
    #[serde(default)]
    pub custom_includes: Vec<String>,
    pub archive: ArchiveMode,
    /// Defaults to `%TEMP%`.
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub run_git_status: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub source: String,
    pub staged: String,
    pub bytes: u64,
    pub modified: i64,
    pub profile: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BackupPlan {
    pub id: String,
    pub staging: String,
    pub items: Vec<PlanItem>,
    pub total_bytes: u64,
    pub file_count: usize,
    pub secret_actions: Vec<String>,
    pub skipped: Vec<SkippedItem>,
    pub warnings: Vec<String>,
    pub archive: ArchiveMode,
    /// Free space on the staging volume, so the UI can refuse an impossible run.
    pub free_bytes: u64,
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// Longest leading path segment with no glob metacharacters.
fn glob_root(pattern: &str) -> PathBuf {
    let norm = pattern.replace('\\', "/");
    let mut root = PathBuf::new();
    let mut first = true;
    for seg in norm.split('/') {
        if seg.contains(['*', '?', '[', '{']) {
            break;
        }
        if first {
            first = false;
            // A leading "/" splits into an empty first segment. Dropping it
            // would silently turn an absolute path into a relative one.
            if seg.is_empty() {
                root.push("/");
                continue;
            }
            if seg.ends_with(':') {
                root.push(format!("{seg}\\")); // "C:" alone means CWD-on-C:
                continue;
            }
        }
        root.push(seg);
    }
    root
}

fn build_set(patterns: &[String]) -> AppResult<globset::GlobSet> {
    let mut b = globset::GlobSetBuilder::new();
    for p in patterns {
        let norm = p.replace('\\', "/");
        // Case-insensitive because Windows is, and `literal_separator` so `*`
        // doesn't leak across directory boundaries.
        let glob = globset::GlobBuilder::new(&norm)
            .case_insensitive(true)
            .literal_separator(true)
            .build()
            .map_err(|e| AppError::Other(format!("bad pattern {p:?}: {e}")))?;
        b.add(glob);
    }
    b.build()
        .map_err(|e| AppError::Other(format!("could not compile pattern set: {e}")))
}

pub fn plan(selection: &BackupSelection, all_profiles: &[Profile]) -> AppResult<BackupPlan> {
    let stamp = timestamp_slug();
    let staging = selection
        .output_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("ReBackUp_{stamp}"));

    let mut items: Vec<PlanItem> = Vec::new();
    let mut skipped: Vec<SkippedItem> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut secret_actions: BTreeSet<String> = BTreeSet::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    let global_excl = build_set(
        &GLOBAL_EXCLUDES
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )?;

    let chosen: Vec<&Profile> = all_profiles
        .iter()
        .filter(|p| selection.profile_ids.iter().any(|id| id == &p.id))
        .collect();

    for profile in &chosen {
        for a in &profile.secrets {
            secret_actions.insert(format!("{a:?}"));
        }
        warnings.extend(profile.notes.iter().cloned());

        let mut includes = profile.expanded_includes();
        if profile.id == "custom" {
            includes.extend(
                selection
                    .custom_includes
                    .iter()
                    .map(|s| crate::util::expand_env(s)),
            );
        }
        let excl = build_set(&profile.expanded_excludes())?;

        for pattern in &includes {
            let root = glob_root(pattern);
            if !root.exists() {
                skipped.push(SkippedItem {
                    path: pattern.clone(),
                    reason: "path does not exist on this machine".into(),
                });
                continue;
            }
            let set = build_set(std::slice::from_ref(pattern))?;
            let literal = !pattern.contains(['*', '?', '[', '{']);

            for entry in walkdir::WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let p = entry.path();
                let norm = p.to_string_lossy().replace('\\', "/");
                if !literal && !set.is_match(norm.as_str()) {
                    continue;
                }
                if excl.is_match(norm.as_str()) || global_excl.is_match(norm.as_str()) {
                    continue;
                }
                push_item(&mut items, &mut seen, &mut skipped, p, &profile.id);
            }
        }
    }

    // Hand-picked files from the scanner ride along under a synthetic profile.
    for extra in &selection.extra_paths {
        let p = PathBuf::from(extra);
        if p.is_file() {
            push_item(&mut items, &mut seen, &mut skipped, &p, "hand-picked");
        } else if p.is_dir() {
            for e in walkdir::WalkDir::new(&p)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
            {
                push_item(&mut items, &mut seen, &mut skipped, e.path(), "hand-picked");
            }
        } else {
            skipped.push(SkippedItem {
                path: extra.clone(),
                reason: "not found".into(),
            });
        }
    }

    // Exclusions are applied last so they always win.
    if !selection.excluded_paths.is_empty() {
        let excl: Vec<String> = selection
            .excluded_paths
            .iter()
            .map(|p| p.replace('\\', "/").to_lowercase())
            .collect();
        items.retain(|i| {
            let s = i.source.replace('\\', "/").to_lowercase();
            !excl
                .iter()
                .any(|e| s == *e || s.starts_with(&format!("{e}/")))
        });
    }

    let total_bytes: u64 = items.iter().map(|i| i.bytes).sum();
    let free_bytes = free_space(&staging);
    if free_bytes > 0 && total_bytes.saturating_mul(2) > free_bytes {
        warnings.push(format!(
            "Staging needs ~{} and the target volume has {} free. Staging plus an archive can \
             use roughly twice the source size — pick another output folder.",
            crate::util::human_bytes(total_bytes),
            crate::util::human_bytes(free_bytes)
        ));
    }

    Ok(BackupPlan {
        id: uuid::Uuid::new_v4().to_string(),
        staging: staging.display().to_string(),
        file_count: items.len(),
        total_bytes,
        items,
        secret_actions: secret_actions.into_iter().collect(),
        skipped,
        warnings,
        archive: selection.archive,
        free_bytes,
    })
}

fn push_item(
    items: &mut Vec<PlanItem>,
    seen: &mut BTreeSet<String>,
    skipped: &mut Vec<SkippedItem>,
    p: &Path,
    profile: &str,
) {
    let key = p.to_string_lossy().to_lowercase();
    if !seen.insert(key) {
        return; // already claimed by an earlier profile
    }
    match std::fs::metadata(p) {
        Ok(m) => items.push(PlanItem {
            source: p.display().to_string(),
            staged: stage_relative(p).to_string_lossy().replace('\\', "/"),
            bytes: m.len(),
            modified: m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            profile: profile.to_string(),
        }),
        Err(e) => skipped.push(SkippedItem {
            path: p.display().to_string(),
            reason: format!("cannot stat: {e}"),
        }),
    }
}

fn free_space(path: &Path) -> u64 {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        match probe.parent() {
            Some(p) => probe = p.to_path_buf(),
            None => return 0,
        }
    }
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| probe.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub plan_id: String,
    pub staging: String,
    pub manifest_path: String,
    pub report_path: String,
    pub restore_script: String,
    pub archive: Option<ArchiveInfo>,
    pub verify: VerifyResult,
    pub files: usize,
    pub bytes: u64,
    pub sealed: usize,
    pub elapsed_ms: u128,
    pub warnings: Vec<String>,
    pub succeeded: bool,
}

pub fn execute(
    app: &AppHandle,
    plan: &BackupPlan,
    all_profiles: &[Profile],
    selection: &BackupSelection,
    pass: &SecretString,
    cancel: Arc<AtomicBool>,
) -> AppResult<BackupResult> {
    let started = std::time::Instant::now();
    let staging = PathBuf::from(&plan.staging);
    std::fs::create_dir_all(staging.join("files"))?;
    std::fs::create_dir_all(staging.join("secrets"))?;

    let log = |level: &'static str, message: String| {
        tracing::info!(target: "backup", "{message}");
        let _ = app.emit(EVT_LOG, LogLine { level, message });
    };
    let progress = |phase: &'static str, done: u64, total: u64, bd: u64, bt: u64, cur: &str| {
        let _ = app.emit(
            EVT_PROGRESS,
            BackupProgress {
                phase,
                done,
                total,
                bytes_done: bd,
                bytes_total: bt,
                current: cur.to_string(),
            },
        );
    };

    let mut manifest = Manifest::new(&staging, selection.profile_ids.clone());
    manifest.warnings = plan.warnings.clone();
    manifest.skipped = plan.skipped.clone();

    // --- 1. stage -----------------------------------------------------------
    log(
        "info",
        format!("Staging {} files into {}", plan.file_count, plan.staging),
    );
    let mut bytes_done = 0u64;
    for (i, item) in plan.items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Other("cancelled by user".into()));
        }
        let dst = staging.join("files").join(&item.staged);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match copy_and_hash(Path::new(&item.source), &dst) {
            Ok((bytes, sha)) => {
                bytes_done += bytes;
                manifest.push(ManifestEntry {
                    source: item.source.clone(),
                    staged: format!("files/{}", item.staged),
                    bytes,
                    sha256: sha,
                    modified: item.modified,
                    profile: item.profile.clone(),
                });
            }
            Err(e) => {
                manifest.skipped.push(SkippedItem {
                    path: item.source.clone(),
                    reason: e.to_string(),
                });
                log("warn", format!("skipped {}: {e}", item.source));
            }
        }
        if i % 25 == 0 || i + 1 == plan.items.len() {
            progress(
                "stage",
                i as u64 + 1,
                plan.items.len() as u64,
                bytes_done,
                plan.total_bytes,
                &item.source,
            );
        }
    }

    // --- 2. secrets ---------------------------------------------------------
    progress("secrets", 0, 1, bytes_done, plan.total_bytes, "");
    let context = run_secret_actions(&staging, all_profiles, selection, pass, &mut manifest, &log)?;
    manifest.context = serde_json::to_value(&context).unwrap_or(serde_json::Value::Null);

    // --- 3. guard rail ------------------------------------------------------
    let problems = manifest.audit_for_plaintext_secrets();
    if !problems.is_empty() {
        return Err(AppError::Integrity(format!(
            "refusing to finish: {}",
            problems.join("; ")
        )));
    }

    // --- 4. restore assets --------------------------------------------------
    let restore_script = crate::restore::write_script(&staging, &manifest)?;
    crate::restore::write_readme(&staging, &manifest)?;
    log(
        "info",
        "Wrote restore.ps1, restore.cmd and READ-ME-FIRST.txt".into(),
    );

    // --- 5. archive ---------------------------------------------------------
    let archive = match plan.archive {
        ArchiveMode::None => None,
        mode => {
            progress("archive", 0, 1, bytes_done, plan.total_bytes, "");
            log("info", "Compressing…".into());
            Some(make_archive(&staging, mode, pass)?)
        }
    };
    manifest.archive = archive.clone();

    // --- 6. verify ----------------------------------------------------------
    log("info", "Verifying hashes…".into());
    let verify = crate::manifest::verify(&manifest, &staging, |d, t| {
        progress(
            "verify",
            d as u64,
            t as u64,
            bytes_done,
            plan.total_bytes,
            "",
        );
    });
    if !verify.passed() {
        log(
            "error",
            format!(
                "Verification FAILED: {} mismatched, {} missing",
                verify.mismatched.len(),
                verify.missing.len()
            ),
        );
    }

    // --- 7. manifest + report ----------------------------------------------
    let manifest_path = manifest.write(&staging)?;
    let report_path = crate::report::write_html(&staging, &manifest, &verify)?;
    log("info", "Done".into());

    let result = BackupResult {
        plan_id: plan.id.clone(),
        staging: plan.staging.clone(),
        manifest_path: manifest_path.display().to_string(),
        report_path: report_path.display().to_string(),
        restore_script: restore_script.display().to_string(),
        archive,
        files: manifest.file_count,
        bytes: manifest.total_bytes,
        sealed: manifest.sealed.len(),
        succeeded: verify.passed(),
        verify,
        elapsed_ms: started.elapsed().as_millis(),
        warnings: manifest.warnings.clone(),
    };
    let _ = app.emit(EVT_DONE, result.clone());
    Ok(result)
}

/// Copy while hashing, so a 30 GB dataset is read once instead of twice.
fn copy_and_hash(src: &Path, dst: &Path) -> AppResult<(u64, String)> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

    let mut input = std::fs::File::open(src)?;
    let mut output = std::io::BufWriter::new(std::fs::File::create(dst)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;

    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n])?;
        total += n as u64;
    }
    output.flush()?;
    // Preserve mtime so the restored tree looks right in Explorer.
    if let Ok(meta) = std::fs::metadata(src) {
        if let Ok(mtime) = meta.modified() {
            let _ = filetime_set(dst, mtime);
        }
    }
    Ok((total, hex::encode(hasher.finalize())))
}

fn filetime_set(path: &Path, mtime: std::time::SystemTime) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new().write(true).open(path)?;
    f.set_modified(mtime)
}

fn run_secret_actions(
    staging: &Path,
    all_profiles: &[Profile],
    selection: &BackupSelection,
    pass: &SecretString,
    manifest: &mut Manifest,
    log: &impl Fn(&'static str, String),
) -> AppResult<serde_json::Value> {
    let chosen: Vec<&Profile> = all_profiles
        .iter()
        .filter(|p| selection.profile_ids.iter().any(|id| id == &p.id))
        .collect();
    let actions: BTreeSet<&SecretAction> = chosen.iter().flat_map(|p| p.secrets.iter()).collect();

    let mut ctx = serde_json::Map::new();

    if actions.contains(&SecretAction::ChromiumPasswords) {
        let browsers = crate::opera::detect_all();
        ctx.insert("browsers".into(), serde_json::to_value(&browsers)?);

        #[cfg(windows)]
        for b in browsers.iter().filter(|b| b.has_login_db && !b.app_bound) {
            let Some(ls) = &b.local_state else { continue };
            let label = format!("{} {}", b.browser, b.profile);
            match crate::secrets::seal_browser_logins(
                staging,
                Path::new(&b.data_dir),
                Path::new(ls),
                &label,
                pass,
            ) {
                Ok((artifact, report)) => {
                    log(
                        "info",
                        format!("Sealed {} logins from {label}", report.decrypted),
                    );
                    manifest.sealed.push(artifact);
                }
                Err(e) => {
                    log("warn", format!("{label}: {e}"));
                    manifest.warnings.push(format!("{label}: {e}"));
                }
            }
        }
        #[cfg(windows)]
        for b in browsers.iter().filter(|b| b.app_bound) {
            manifest.warnings.push(format!(
                "{} {} uses app-bound encryption. Export its passwords manually from {} and \
                 add the CSV on the Review step — it will be sealed and the plaintext shredded.",
                b.browser,
                b.profile,
                crate::opera::password_manager_url(&b.browser)
            ));
        }
    }

    if actions.contains(&SecretAction::SteamSentry) {
        let report = crate::steam::detect()?;
        for w in &report.warnings {
            manifest.warnings.push(format!("Steam: {w}"));
        }
        for (i, sentry) in report.sentry_files.iter().enumerate() {
            match crate::secrets::seal_file(
                staging,
                Path::new(sentry),
                &format!("secrets/steam-sentry-{i}.rbu"),
                "Steam sentry file",
                pass,
            ) {
                Ok(a) => manifest.sealed.push(a),
                Err(e) => log("warn", format!("steam sentry: {e}")),
            }
        }
        ctx.insert("steam".into(), serde_json::to_value(&report)?);
    }

    if actions.contains(&SecretAction::GitCredentials) {
        let roots: Vec<PathBuf> = dirs::home_dir().into_iter().collect();
        let report = crate::git::discover(&roots, 8, selection.run_git_status)?;
        for w in &report.warnings {
            manifest.warnings.push(format!("Git: {w}"));
        }
        if let Some(cred) = &report.git_credentials_file {
            match crate::secrets::seal_file(
                staging,
                Path::new(cred),
                "secrets/git-credentials.rbu",
                "git-credentials (plaintext tokens)",
                pass,
            ) {
                Ok(a) => manifest.sealed.push(a),
                Err(e) => log("warn", format!(".git-credentials: {e}")),
            }
        }
        for key in report.ssh_keys.iter().filter(|k| !k.ends_with(".pub")) {
            let name = Path::new(key)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "key".into());
            match crate::secrets::seal_file(
                staging,
                Path::new(key),
                &format!("secrets/ssh-{name}.rbu"),
                &format!("SSH key {name}"),
                pass,
            ) {
                Ok(a) => manifest.sealed.push(a),
                Err(e) => log("warn", format!("ssh key {name}: {e}")),
            }
        }
        ctx.insert("git".into(), serde_json::to_value(&report)?);
    }

    #[cfg(windows)]
    if actions.contains(&SecretAction::WindowsVault) {
        match crate::secrets::vault::enumerate() {
            Ok(creds) => {
                log(
                    "info",
                    format!("Inventoried {} stored credentials", creds.len()),
                );
                ctx.insert("credentials".into(), serde_json::to_value(&creds)?);
            }
            Err(e) => manifest.warnings.push(format!("Credential Manager: {e}")),
        }
        // A .crd the user produced with the wizard while this ran.
        if let Some(crd) = crate::secrets::vault::find_recent_crd(staging, 3600) {
            manifest.warnings.push(format!(
                "Found {} — it is already password-protected by Windows; keep that password safe.",
                crd.display()
            ));
        } else {
            manifest.warnings.push(
                "No .crd export was found. Windows Credential Manager entries will NOT be \
                 restored unless you run the guided export."
                    .into(),
            );
        }
    }

    Ok(serde_json::Value::Object(ctx))
}

fn make_archive(staging: &Path, mode: ArchiveMode, _pass: &SecretString) -> AppResult<ArchiveInfo> {
    let name = staging
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "backup".into());
    let parent = staging.parent().unwrap_or(Path::new("."));

    match mode {
        ArchiveMode::None => unreachable!(),

        #[cfg(feature = "sevenz")]
        ArchiveMode::SevenZip => {
            let dst = parent.join(format!("{name}.7z"));
            // AES-256 with a key derived from the passphrase by 7-Zip's own KDF
            // (SHA-256, 2^19 iterations). The sealed blobs inside stay sealed
            // regardless, so this is defence in depth, not the only layer.
            sevenz_rust2::compress_to_path_encrypted(
                staging,
                &dst,
                sevenz_rust2::Password::from(_pass.expose()),
            )
            .map_err(|e| AppError::Other(format!("7z compression failed: {e}")))?;
            Ok(ArchiveInfo {
                bytes: std::fs::metadata(&dst)?.len(),
                sha256: crate::crypto::sha256_file(&dst)?,
                path: dst.display().to_string(),
                format: "7z/LZMA2".into(),
                encrypted: true,
                cipher: Some("aes-256-cbc (7-Zip)".into()),
            })
        }

        #[cfg(not(feature = "sevenz"))]
        ArchiveMode::SevenZip => Err(AppError::Other(
            "this build was compiled without the `sevenz` feature; choose Zip or None".into(),
        )),

        ArchiveMode::Zip => {
            let dst = parent.join(format!("{name}.zip"));
            zip_dir(staging, &dst)?;
            Ok(ArchiveInfo {
                bytes: std::fs::metadata(&dst)?.len(),
                sha256: crate::crypto::sha256_file(&dst)?,
                path: dst.display().to_string(),
                format: "zip/zstd".into(),
                // Legacy ZipCrypto is broken and zip2 AES support is uneven, so
                // we do not claim encryption we aren't providing.
                encrypted: false,
                cipher: None,
            })
        }
    }
}

fn zip_dir(src: &Path, dst: &Path) -> AppResult<()> {
    use std::io::Write;
    let file = std::fs::File::create(dst)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Zstd);

    for e in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(Result::ok)
    {
        let rel = match e.path().strip_prefix(src) {
            Ok(r) if !r.as_os_str().is_empty() => r,
            _ => continue,
        };
        let name = rel.to_string_lossy().replace('\\', "/");
        if e.file_type().is_dir() {
            zip.add_directory(name, opts)?;
        } else if e.file_type().is_file() {
            zip.start_file(name, opts)?;
            let mut f = std::fs::File::open(e.path())?;
            std::io::copy(&mut f, &mut zip)?;
        }
    }
    zip.finish()?.flush()?;
    Ok(())
}

impl From<zip::result::ZipError> for AppError {
    fn from(e: zip::result::ZipError) -> Self {
        AppError::Other(format!("zip: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_root_stops_at_the_first_wildcard() {
        assert_eq!(
            glob_root(r"C:/Users/a/Desktop/**"),
            PathBuf::from(r"C:\")
                .join("Users")
                .join("a")
                .join("Desktop")
        );
        assert_eq!(glob_root("/home/a/.ssh"), PathBuf::from("/home/a/.ssh"));
    }

    #[test]
    fn glob_matching_is_case_insensitive_and_separator_aware() {
        let set = build_set(&["C:/Users/**/Desktop/*.txt".to_string()]).unwrap();
        assert!(set.is_match("c:/users/bob/Desktop/a.txt"));
        assert!(!set.is_match("c:/users/bob/Desktop/sub/a.txt"));
    }

    #[test]
    fn copy_and_hash_matches_a_separate_hash_pass() {
        let dir = std::env::temp_dir().join(format!("rbu-cp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("in.bin");
        let dst = dir.join("out.bin");
        let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &payload).unwrap();

        let (n, sha) = copy_and_hash(&src, &dst).unwrap();
        assert_eq!(n, payload.len() as u64);
        assert_eq!(sha, crate::crypto::sha256_bytes(&payload));
        assert_eq!(sha, crate::crypto::sha256_file(&dst).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_deduplicates_paths_claimed_by_two_profiles() {
        let dir = std::env::temp_dir().join(format!("rbu-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();
        let pat = format!("{}/**", dir.display().to_string().replace('\\', "/"));

        let mk = |id: &str| Profile {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            category: crate::profiles::Category::Custom,
            include: vec![pat.clone()],
            exclude: vec![],
            secrets: vec![],
            enabled_by_default: false,
            builtin: false,
            notes: vec![],
        };
        let all = vec![mk("one"), mk("two")];
        let sel = BackupSelection {
            profile_ids: vec!["one".into(), "two".into()],
            extra_paths: vec![],
            excluded_paths: vec![],
            custom_includes: vec![],
            archive: ArchiveMode::None,
            output_dir: Some(dir.display().to_string()),
            run_git_status: false,
        };
        let p = plan(&sel, &all).unwrap();
        assert_eq!(p.file_count, 1, "a file claimed twice must be staged once");
        assert_eq!(p.items[0].profile, "one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn explicit_exclusions_override_includes() {
        let dir = std::env::temp_dir().join(format!("rbu-plan2-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("keep")).unwrap();
        std::fs::create_dir_all(dir.join("drop")).unwrap();
        std::fs::write(dir.join("keep/a.txt"), b"a").unwrap();
        std::fs::write(dir.join("drop/b.txt"), b"b").unwrap();

        let pat = format!("{}/**", dir.display().to_string().replace('\\', "/"));
        let all = vec![Profile {
            id: "p".into(),
            name: "p".into(),
            description: String::new(),
            category: crate::profiles::Category::Custom,
            include: vec![pat],
            exclude: vec![],
            secrets: vec![],
            enabled_by_default: false,
            builtin: false,
            notes: vec![],
        }];
        let sel = BackupSelection {
            profile_ids: vec!["p".into()],
            extra_paths: vec![],
            excluded_paths: vec![dir.join("drop").display().to_string()],
            custom_includes: vec![],
            archive: ArchiveMode::None,
            output_dir: Some(dir.display().to_string()),
            run_git_status: false,
        };
        let p = plan(&sel, &all).unwrap();
        assert_eq!(p.file_count, 1);
        assert!(p.items[0].source.contains("keep"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
