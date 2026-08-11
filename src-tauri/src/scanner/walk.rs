//! Portable fallback scanner.
//!
//! Used when (a) we're not on Windows, (b) the volume isn't NTFS, or (c) the
//! process isn't elevated enough to open the raw volume. Roughly 10-40x slower
//! than the MFT path on a large volume, but it needs no privileges and works
//! on ReFS/exFAT/FAT32/ext4/APFS.
//!
//! Parallelism comes from `ignore::WalkBuilder`, which fans directory reads out
//! across a thread pool. Results are funnelled back through a channel so the
//! index stays single-writer (no locking in the hot path).

use crate::scanner::index::{IndexBuilder, ScanIndex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

struct Found {
    path: PathBuf,
    size: u64,
    mtime: i64,
    is_dir: bool,
}

/// Walk `root`, returning a finished index. `progress(files_seen, 0)` fires
/// every ~4096 entries (total is unknown up front, hence the 0).
pub fn scan<P, C>(root: &Path, threads: usize, mut progress: P, cancel: C) -> (ScanIndex, bool)
where
    P: FnMut(u64, u64),
    C: Fn() -> bool + Send + Sync + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<Found>();
    let seen = Arc::new(AtomicU64::new(0));
    let aborted = Arc::new(AtomicBool::new(false));

    let walker = ignore::WalkBuilder::new(root)
        .hidden(false) // we WANT dotfiles and hidden files
        .git_ignore(false) // .gitignore must not hide backup candidates
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .follow_links(false) // never follow symlinks: cycle + escape risk
        .same_file_system(true) // don't wander into mounted volumes
        .threads(threads.max(1))
        .build_parallel();

    let cancel = Arc::new(cancel);
    {
        let seen = Arc::clone(&seen);
        let aborted = Arc::clone(&aborted);
        let cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            // `move` matters: the builder must OWN its Sender. `&Sender<T>` is
            // not Send (Sender isn't Sync), so borrowing it here would not
            // compile once `ignore` hands the builder to its worker threads.
            walker.run(move || {
                let tx = tx.clone();
                let seen = Arc::clone(&seen);
                let aborted = Arc::clone(&aborted);
                let cancel = Arc::clone(&cancel);
                Box::new(move |entry| {
                    if cancel() {
                        aborted.store(true, Ordering::Relaxed);
                        return ignore::WalkState::Quit;
                    }
                    let Ok(entry) = entry else {
                        // Permission denied on a subtree is normal; skip it.
                        return ignore::WalkState::Continue;
                    };
                    let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
                    let (size, mtime) = match entry.metadata() {
                        Ok(m) => (
                            if is_dir { 0 } else { m.len() },
                            m.modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0),
                        ),
                        Err(_) => (0, 0),
                    };
                    seen.fetch_add(1, Ordering::Relaxed);
                    let _ = tx.send(Found {
                        path: entry.into_path(),
                        size,
                        mtime,
                        is_dir,
                    });
                    ignore::WalkState::Continue
                })
            });
            // `tx` clones drop with the closures; the channel closes here.
        });
    }

    // --- single-writer index construction -----------------------------------
    let mut b = IndexBuilder::with_capacity(1 << 16);
    let root_id = b.push(0, "", 0, 0, true);
    let mut by_path: HashMap<PathBuf, u32> = HashMap::with_capacity(1 << 14);
    by_path.insert(root.to_path_buf(), root_id);

    let mut count = 0u64;
    for f in rx {
        if f.path == root {
            continue;
        }
        let parent_id = match f.path.parent() {
            Some(p) => *by_path.get(p).unwrap_or(&root_id),
            None => root_id,
        };
        let name = f
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let id = b.push(parent_id, &name, f.size, f.mtime, f.is_dir);
        if f.is_dir {
            by_path.insert(f.path, id);
        }
        count += 1;
        if count % 4096 == 0 {
            progress(count, 0);
        }
    }
    progress(count, count);

    let volume = volume_prefix(root);
    (b.build(volume, root_id), !aborted.load(Ordering::Relaxed))
}

/// `C:\` -> `C:`; POSIX roots produce an empty prefix.
fn volume_prefix(root: &Path) -> String {
    let s = root.to_string_lossy();
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        s[..2].to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn indexes_a_temp_tree() {
        let dir = std::env::temp_dir().join(format!("prb-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let mut f = std::fs::File::create(dir.join("sub").join("a.txt")).unwrap();
        f.write_all(&[0u8; 1234]).unwrap();
        drop(f);

        let (ix, completed) = scan(&dir, 2, |_, _| {}, || false);
        assert!(completed);
        assert_eq!(ix.file_count, 1);
        assert_eq!(ix.bytes_total, 1234);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
