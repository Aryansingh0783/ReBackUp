//! ReBackUp — library root.
//!
//! Everything the UI can do goes through a `#[tauri::command]` in this file.
//! Long-running work (scanning, copying, hashing, compressing) is pushed onto
//! blocking threads and reports back via events, so the webview never stalls.

pub mod backup;
pub mod cli;
pub mod crypto;
pub mod detect;
pub mod error;
pub mod git;
pub mod manifest;
pub mod opera;
pub mod profiles;
pub mod report;
pub mod restore;
pub mod scanner;
pub mod secrets;
pub mod steam;
pub mod util;
pub mod vdf;

use crate::crypto::SecretString;
use crate::error::{AppError, AppResult};
use parking_lot::RwLock;
use scanner::index::{FileFilter, QueryResult, SortKey, TreeNode};
use scanner::{ScanOptions, ScanStore, ScanSummary};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct AppState {
    scans: Arc<ScanStore>,
    plans: RwLock<HashMap<String, backup::BackupPlan>>,
    backup_cancel: Arc<AtomicBool>,
}

// ---------------------------------------------------------------------------
// Drives + environment
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub name: String,
    pub mount: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub removable: bool,
    /// True when the raw-MFT fast path is possible on this volume.
    pub ntfs: bool,
}

#[tauri::command]
fn list_drives() -> Vec<DriveInfo> {
    sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .map(|d| {
            let fs = d.file_system().to_string_lossy().to_string();
            DriveInfo {
                name: d.name().to_string_lossy().to_string(),
                mount: d.mount_point().to_string_lossy().to_string(),
                ntfs: fs.eq_ignore_ascii_case("ntfs"),
                file_system: fs,
                total_bytes: d.total_space(),
                free_bytes: d.available_space(),
                removable: d.is_removable(),
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub elevated: bool,
    pub windows: bool,
    pub mft_available: bool,
    pub temp_dir: String,
    pub home_dir: String,
    pub user: String,
    pub version: String,
}

#[tauri::command]
fn environment() -> Environment {
    #[cfg(windows)]
    let elevated = scanner::mft::is_elevated();
    #[cfg(not(windows))]
    let elevated = false;

    Environment {
        elevated,
        windows: cfg!(windows),
        mft_available: cfg!(windows) && elevated,
        temp_dir: std::env::temp_dir().display().to_string(),
        home_dir: dirs::home_dir().unwrap_or_default().display().to_string(),
        user: std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

#[tauri::command]
fn start_scan(app: AppHandle, state: State<'_, AppState>, options: ScanOptions) -> String {
    scanner::start(app, Arc::clone(&state.scans), options)
}

#[tauri::command]
fn set_scan_paused(state: State<'_, AppState>, scan_id: String, paused: bool) -> AppResult<()> {
    let ctls = state.scans.controls.read();
    let ctl = ctls
        .get(&scan_id)
        .ok_or_else(|| AppError::UnknownScan(scan_id.clone()))?;
    ctl.paused.store(paused, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>, scan_id: String) -> AppResult<()> {
    if let Some(ctl) = state.scans.controls.read().get(&scan_id) {
        ctl.cancel.store(true, Ordering::Relaxed);
        // Un-pause so the worker can observe the cancel and exit.
        ctl.paused.store(false, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
fn scan_summary(state: State<'_, AppState>, scan_id: String) -> AppResult<ScanSummary> {
    state
        .scans
        .summaries
        .read()
        .get(&scan_id)
        .cloned()
        .ok_or(AppError::UnknownScan(scan_id))
}

#[tauri::command]
fn scan_tree(
    state: State<'_, AppState>,
    scan_id: String,
    node: Option<u32>,
    depth: Option<u32>,
    fanout: Option<usize>,
) -> AppResult<TreeNode> {
    let ix = state.scans.get(&scan_id)?;
    let node = node.unwrap_or(ix.root);
    Ok(ix.tree(node, depth.unwrap_or(2), fanout.unwrap_or(24)))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn query_files(
    state: State<'_, AppState>,
    scan_id: String,
    filter: FileFilter,
    sort: Option<SortKey>,
    desc: Option<bool>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> AppResult<QueryResult> {
    let ix = state.scans.get(&scan_id)?;
    Ok(ix.query(
        &filter,
        sort.unwrap_or_default(),
        desc.unwrap_or(true),
        offset.unwrap_or(0),
        limit.unwrap_or(500).min(5000),
    ))
}

/// Find `.git` directories in an existing scan — effectively free, because the
/// index already knows every directory on the volume.
#[tauri::command]
fn git_repos_from_scan(
    state: State<'_, AppState>,
    scan_id: String,
    run_status: bool,
    limit: Option<usize>,
) -> AppResult<git::GitReport> {
    let ix = state.scans.get(&scan_id)?;
    let mut paths = Vec::new();
    for i in 0..ix.len() {
        let id = i as u32;
        if ix.is_dir(id) && ix.is_live(id) && &*ix.name[i] == ".git" {
            paths.push(ix.path_of(id));
            if paths.len() >= limit.unwrap_or(2000) {
                break;
            }
        }
    }
    Ok(git::from_paths(&paths, run_status))
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_profiles() -> Vec<profiles::Profile> {
    profiles::load()
}

#[tauri::command]
fn save_profiles(profiles: Vec<profiles::Profile>) -> AppResult<()> {
    profiles::save(&profiles)
}

#[tauri::command]
fn detect_targets() -> Vec<detect::Detection> {
    detect::all()
}

#[tauri::command]
fn detect_browsers() -> Vec<opera::BrowserProfile> {
    opera::detect_all()
}

#[tauri::command]
fn detect_steam() -> AppResult<steam::SteamReport> {
    steam::detect()
}

#[tauri::command]
fn discover_git(roots: Vec<String>, run_status: bool) -> AppResult<git::GitReport> {
    let roots: Vec<std::path::PathBuf> = if roots.is_empty() {
        dirs::home_dir().into_iter().collect()
    } else {
        roots.into_iter().map(std::path::PathBuf::from).collect()
    };
    git::discover(&roots, 10, run_status)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub credentials: Vec<serde_json::Value>,
    pub steps: Vec<&'static str>,
    pub supported: bool,
    pub message: Option<String>,
}

#[tauri::command]
fn credential_manager_info() -> VaultInfo {
    #[cfg(windows)]
    {
        match secrets::vault::enumerate() {
            Ok(c) => VaultInfo {
                credentials: c
                    .into_iter()
                    .filter_map(|x| serde_json::to_value(x).ok())
                    .collect(),
                steps: secrets::vault::WIZARD_STEPS.to_vec(),
                supported: true,
                message: None,
            },
            Err(e) => VaultInfo {
                credentials: vec![],
                steps: secrets::vault::WIZARD_STEPS.to_vec(),
                supported: true,
                message: Some(e.to_string()),
            },
        }
    }
    #[cfg(not(windows))]
    {
        VaultInfo {
            credentials: vec![],
            steps: vec![],
            supported: false,
            message: Some("Windows Credential Manager only exists on Windows.".into()),
        }
    }
}

#[tauri::command]
fn open_credential_wizard() -> AppResult<()> {
    #[cfg(windows)]
    {
        secrets::vault::open_backup_wizard()
    }
    #[cfg(not(windows))]
    {
        Err(AppError::WindowsOnly("Credential Manager"))
    }
}

#[tauri::command]
fn password_manager_url(browser: String) -> String {
    opera::password_manager_url(&browser).to_string()
}

// ---------------------------------------------------------------------------
// Backup
// ---------------------------------------------------------------------------

#[tauri::command]
fn check_passphrase(passphrase: String) -> AppResult<()> {
    SecretString::new(passphrase).check_strength()
}

#[tauri::command]
fn plan_backup(
    state: State<'_, AppState>,
    selection: backup::BackupSelection,
) -> AppResult<backup::BackupPlan> {
    let all = profiles::load();
    let plan = backup::plan(&selection, &all)?;
    state.plans.write().insert(plan.id.clone(), plan.clone());
    Ok(plan)
}

#[tauri::command]
async fn run_backup(
    app: AppHandle,
    plan_id: String,
    selection: backup::BackupSelection,
    passphrase: String,
) -> AppResult<backup::BackupResult> {
    // Scope the `State` borrow so nothing tied to `app` is held across the
    // `.await` below — that keeps this future unambiguously `Send`.
    let (plan, cancel) = {
        let state = app.state::<AppState>();
        let plan = state
            .plans
            .read()
            .get(&plan_id)
            .cloned()
            .ok_or_else(|| AppError::Other(format!("no plan {plan_id}")))?;
        (plan, Arc::clone(&state.backup_cancel))
    };
    cancel.store(false, Ordering::Relaxed);

    let app2 = app.clone();
    // Staging + hashing + compression are CPU/IO bound: keep them off the async
    // runtime's worker threads.
    tauri::async_runtime::spawn_blocking(move || {
        let pass = SecretString::new(passphrase);
        let all = profiles::load();
        backup::execute(&app2, &plan, &all, &selection, &pass, cancel)
    })
    .await
    .map_err(|e| AppError::Other(format!("backup task panicked: {e}")))?
}

#[tauri::command]
fn cancel_backup(state: State<'_, AppState>) {
    state.backup_cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
fn verify_backup(manifest_path: String) -> AppResult<manifest::VerifyResult> {
    let path = std::path::PathBuf::from(&manifest_path);
    let m = manifest::Manifest::read(&path)?;
    let staging = path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    Ok(manifest::verify(&m, &staging, |_, _| {}))
}

/// Seal a CSV the user exported by hand (app-bound-encryption path).
#[tauri::command]
fn seal_exported_csv(
    staging: String,
    csv_path: String,
    label: String,
    passphrase: String,
    shred_source: bool,
) -> AppResult<secrets::SealedArtifact> {
    let pass = SecretString::new(passphrase);
    pass.check_strength()?;
    secrets::seal_exported_csv(
        std::path::Path::new(&staging),
        std::path::Path::new(&csv_path),
        &label,
        &pass,
        shred_source,
    )
}

#[tauri::command]
fn unseal_to(sealed_path: String, out_path: String, passphrase: String) -> AppResult<u64> {
    let pass = SecretString::new(passphrase);
    let plain = secrets::open_sealed(std::path::Path::new(&sealed_path), &pass)?;
    std::fs::write(&out_path, &plain[..])?;
    Ok(plain.len() as u64)
}

#[tauri::command]
fn shred_file(path: String) -> AppResult<()> {
    secrets::shred(std::path::Path::new(&path))
}

#[tauri::command]
fn read_manifest(path: String) -> AppResult<manifest::Manifest> {
    manifest::Manifest::read(std::path::Path::new(&path))
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("REBACKUP_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    // CLI mode: `rebackup unseal|verify|shred ...`. Used by restore.ps1
    // on the fresh install, where there may be no GUI session yet.
    if let Some(code) = cli::maybe_run() {
        std::process::exit(code);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            environment,
            start_scan,
            set_scan_paused,
            cancel_scan,
            scan_summary,
            scan_tree,
            query_files,
            git_repos_from_scan,
            list_profiles,
            save_profiles,
            detect_targets,
            detect_browsers,
            detect_steam,
            discover_git,
            credential_manager_info,
            open_credential_wizard,
            password_manager_url,
            check_passphrase,
            plan_backup,
            run_backup,
            cancel_backup,
            verify_backup,
            seal_exported_csv,
            unseal_to,
            shred_file,
            read_manifest,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
