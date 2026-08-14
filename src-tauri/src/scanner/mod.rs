//! Scan orchestration: pick a backend, run it off the UI thread, stream
//! progress events, and park the finished index in shared state.

pub mod index;
#[cfg(windows)]
pub mod mft;
pub mod walk;

use crate::error::{AppError, AppResult};
use index::ScanIndex;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub const EVT_PROGRESS: &str = "scan://progress";
pub const EVT_DONE: &str = "scan://done";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanBackend {
    /// Raw MFT. Windows + NTFS + elevated.
    Mft,
    /// Portable directory walk.
    Walk,
    /// Try MFT, silently fall back to walk.
    Auto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    /// `C:` (MFT) or any directory path (walk).
    pub target: String,
    #[serde(default = "default_backend")]
    pub backend: ScanBackend,
    #[serde(default)]
    pub threads: usize,
}

fn default_backend() -> ScanBackend {
    ScanBackend::Auto
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scan_id: String,
    pub done: u64,
    pub total: u64,
    pub phase: &'static str,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub scan_id: String,
    pub target: String,
    pub backend: &'static str,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub elapsed_ms: u128,
    pub completed: bool,
    pub root: u32,
    /// Set when the MFT path was attempted and refused; the UI shows an
    /// "elevate for a 20x faster scan" hint.
    pub fallback_reason: Option<String>,
}

/// Cancellation / pause flags shared with the worker thread.
pub struct ScanControl {
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl ScanControl {
    fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
        }
    }

    /// Blocks while paused; returns true if the caller should abort.
    fn should_stop(&self) -> bool {
        while self.paused.load(Ordering::Relaxed) && !self.cancel.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
pub struct ScanStore {
    pub scans: RwLock<HashMap<String, Arc<ScanIndex>>>,
    pub controls: RwLock<HashMap<String, Arc<ScanControl>>>,
    pub summaries: RwLock<HashMap<String, ScanSummary>>,
}

impl ScanStore {
    pub fn get(&self, id: &str) -> AppResult<Arc<ScanIndex>> {
        self.scans
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::UnknownScan(id.to_string()))
    }
}

/// Kick off a scan on a background thread and return its id immediately.
pub fn start(app: AppHandle, store: Arc<ScanStore>, opts: ScanOptions) -> String {
    let scan_id = uuid::Uuid::new_v4().to_string();
    let ctl = Arc::new(ScanControl::new());
    store.controls.write().insert(scan_id.clone(), ctl.clone());

    let id2 = scan_id.clone();
    std::thread::Builder::new()
        .name("rbu-scan".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            let (index, completed, backend, fallback) = run(&app, &id2, &ctl, &opts);

            let summary = ScanSummary {
                scan_id: id2.clone(),
                target: opts.target.clone(),
                backend,
                files: index.file_count,
                dirs: index.dir_count,
                bytes: index.bytes_total,
                elapsed_ms: started.elapsed().as_millis(),
                completed,
                root: index.root,
                fallback_reason: fallback,
            };
            store.scans.write().insert(id2.clone(), Arc::new(index));
            store.summaries.write().insert(id2.clone(), summary.clone());
            store.controls.write().remove(&id2);
            let _ = app.emit(EVT_DONE, summary);
        })
        .expect("failed to spawn scan thread");

    scan_id
}

fn run(
    app: &AppHandle,
    scan_id: &str,
    ctl: &Arc<ScanControl>,
    opts: &ScanOptions,
) -> (ScanIndex, bool, &'static str, Option<String>) {
    let emit = |phase: &'static str, done: u64, total: u64| {
        ctl.done.store(done, Ordering::Relaxed);
        ctl.total.store(total, Ordering::Relaxed);
        let _ = app.emit(
            EVT_PROGRESS,
            ScanProgress {
                scan_id: scan_id.to_string(),
                done,
                total,
                phase,
            },
        );
    };

    #[cfg(windows)]
    if matches!(opts.backend, ScanBackend::Mft | ScanBackend::Auto) {
        let letter = opts
            .target
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or('C');

        match try_mft(letter, ctl, &emit) {
            Ok(Some(ix)) => {
                return (ix, !ctl.cancel.load(Ordering::Relaxed), "mft", None);
            }
            // Cancelled mid-scan: return empty rather than silently restarting
            // the whole volume with the slow backend.
            Ok(None) => return (ScanIndex::default(), false, "mft", None),
            Err(e) => {
                if matches!(opts.backend, ScanBackend::Mft) {
                    tracing::warn!("MFT scan failed and no fallback was requested: {e}");
                    return (ScanIndex::default(), false, "mft", Some(e.to_string()));
                }
                tracing::info!("MFT unavailable, falling back to walkdir: {e}");
                let reason = e.to_string();
                let root = std::path::PathBuf::from(format!("{letter}:\\"));
                let ctl2 = Arc::clone(ctl);
                let (ix, completed) = walk::scan(
                    &root,
                    threads(opts),
                    |d, t| emit("walk", d, t),
                    move || ctl2.should_stop(),
                );
                return (ix, completed, "walk", Some(reason));
            }
        }
    }

    let root = std::path::PathBuf::from(&opts.target);
    let ctl2 = Arc::clone(ctl);
    let (ix, completed) = walk::scan(
        &root,
        threads(opts),
        |d, t| emit("walk", d, t),
        move || ctl2.should_stop(),
    );
    (ix, completed, "walk", None)
}

fn threads(opts: &ScanOptions) -> usize {
    if opts.threads > 0 {
        opts.threads
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    }
}

#[cfg(windows)]
fn try_mft(
    letter: char,
    ctl: &Arc<ScanControl>,
    emit: &impl Fn(&'static str, u64, u64),
) -> AppResult<Option<ScanIndex>> {
    // MFT record numbers are dense and become node ids verbatim, so a plain
    // `put` per entry needs no name->id map at all.
    let mut b = index::IndexBuilder::with_capacity(1 << 18);
    let ctl2 = Arc::clone(ctl);

    let completed = mft::enumerate(
        letter,
        |e| {
            b.put(e.record, e.parent, &e.name, e.size, e.mtime, e.is_dir);
        },
        |done, total| emit("mft", done, total),
        || ctl2.should_stop(),
    )?;

    if !completed {
        return Ok(None);
    }
    emit("index", 1, 1);
    Ok(Some(b.build(format!("{letter}:"), mft::ROOT_RECORD)))
}
