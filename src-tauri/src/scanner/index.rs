//! Compact, cache-friendly index of a scanned volume.
//!
//! A 1 TB NVMe can hold 3M+ files. Storing a `String` full path per entry would
//! cost ~250 MB of heap and murder cache locality, so instead we keep a
//! struct-of-arrays keyed by node id and reconstruct paths on demand by walking
//! `parent` links. On the MFT path the node id *is* the MFT record number,
//! which makes parent resolution a single array index.
//!
//! Children are stored in CSR (compressed sparse row) form: one `child_off`
//! offset array plus one flat `child_idx` array, built once in `finalize`.

use serde::{Deserialize, Serialize};

pub const FLAG_DIR: u8 = 0b0000_0001;
pub const FLAG_LIVE: u8 = 0b0000_0010;

/// Guards against parent-pointer cycles from a corrupt or racing MFT.
const MAX_DEPTH: usize = 512;

#[derive(Default)]
pub struct ScanIndex {
    /// Volume prefix, e.g. `C:`. Empty for a rooted POSIX scan.
    pub volume: String,
    pub root: u32,
    pub name: Vec<Box<str>>,
    pub parent: Vec<u32>,
    /// Own size in bytes. Directories are 0 here.
    pub size: Vec<u64>,
    /// Subtree size, filled in by [`ScanIndex::finalize`].
    pub total: Vec<u64>,
    pub mtime: Vec<i64>,
    pub flags: Vec<u8>,
    pub child_off: Vec<u32>,
    pub child_idx: Vec<u32>,
    pub file_count: u64,
    pub dir_count: u64,
    pub bytes_total: u64,
}

impl ScanIndex {
    #[inline]
    pub fn len(&self) -> usize {
        self.name.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }
    #[inline]
    pub fn is_live(&self, i: u32) -> bool {
        self.flags.get(i as usize).is_some_and(|f| f & FLAG_LIVE != 0)
    }
    #[inline]
    pub fn is_dir(&self, i: u32) -> bool {
        self.flags.get(i as usize).is_some_and(|f| f & FLAG_DIR != 0)
    }

    /// Reconstruct the absolute path of `i` by walking to the root.
    pub fn path_of(&self, i: u32) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(16);
        let mut cur = i;
        let mut depth = 0;
        while cur != self.root && cur != u32::MAX && depth < MAX_DEPTH {
            let Some(n) = self.name.get(cur as usize) else { break };
            parts.push(n);
            let Some(&p) = self.parent.get(cur as usize) else { break };
            if p == cur {
                break; // self-parent: corrupt record
            }
            cur = p;
            depth += 1;
        }
        parts.reverse();

        let mut out = String::with_capacity(self.volume.len() + parts.len() * 12 + 2);
        if self.volume.is_empty() {
            out.push('/');
            out.push_str(&parts.join("/"));
        } else {
            out.push_str(&self.volume);
            out.push('\\');
            out.push_str(&parts.join("\\"));
        }
        out
    }

    pub fn children(&self, i: u32) -> &[u32] {
        let i = i as usize;
        if i + 1 >= self.child_off.len() {
            return &[];
        }
        let a = self.child_off[i] as usize;
        let b = self.child_off[i + 1] as usize;
        &self.child_idx[a..b]
    }

    /// Build the CSR child index and roll subtree totals up to every ancestor.
    ///
    /// Totals are accumulated by walking each file's ancestor chain rather than
    /// by a topological sort: depth is ~10 in practice, so this is O(n·depth)
    /// with a tiny constant and no extra allocation.
    pub fn finalize(&mut self) {
        let n = self.len();
        self.total = vec![0; n];

        // --- CSR children -----------------------------------------------
        let mut counts = vec![0u32; n + 1];
        for i in 0..n {
            if self.flags[i] & FLAG_LIVE == 0 || i as u32 == self.root {
                continue;
            }
            let p = self.parent[i];
            if (p as usize) < n {
                counts[p as usize] += 1;
            }
        }
        self.child_off = vec![0u32; n + 1];
        let mut acc = 0u32;
        for i in 0..n {
            self.child_off[i] = acc;
            acc += counts[i];
        }
        self.child_off[n] = acc;

        let mut cursor = self.child_off.clone();
        self.child_idx = vec![0u32; acc as usize];
        for i in 0..n {
            if self.flags[i] & FLAG_LIVE == 0 || i as u32 == self.root {
                continue;
            }
            let p = self.parent[i] as usize;
            if p < n {
                self.child_idx[cursor[p] as usize] = i as u32;
                cursor[p] += 1;
            }
        }

        // --- totals + counters -------------------------------------------
        self.file_count = 0;
        self.dir_count = 0;
        self.bytes_total = 0;
        for i in 0..n {
            if self.flags[i] & FLAG_LIVE == 0 {
                continue;
            }
            if self.flags[i] & FLAG_DIR != 0 {
                self.dir_count += 1;
                continue;
            }
            self.file_count += 1;
            let sz = self.size[i];
            self.bytes_total += sz;
            self.total[i] += sz;

            let mut cur = self.parent[i];
            let mut depth = 0;
            while (cur as usize) < n && depth < MAX_DEPTH {
                self.total[cur as usize] += sz;
                if cur == self.root {
                    break;
                }
                let next = self.parent[cur as usize];
                if next == cur {
                    break;
                }
                cur = next;
                depth += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct IndexBuilder {
    idx: ScanIndex,
}

impl IndexBuilder {
    pub fn with_capacity(cap: usize) -> Self {
        let mut idx = ScanIndex::default();
        idx.name.reserve(cap);
        idx.parent.reserve(cap);
        idx.size.reserve(cap);
        idx.mtime.reserve(cap);
        idx.flags.reserve(cap);
        Self { idx }
    }

    fn ensure(&mut self, i: usize) {
        if self.idx.name.len() <= i {
            let need = i + 1;
            self.idx.name.resize_with(need, || "".into());
            self.idx.parent.resize(need, u32::MAX);
            self.idx.size.resize(need, 0);
            self.idx.mtime.resize(need, 0);
            self.idx.flags.resize(need, 0);
        }
    }

    /// Place a node at an externally chosen id (MFT record number).
    pub fn put(&mut self, id: u32, parent: u32, name: &str, size: u64, mtime: i64, is_dir: bool) {
        self.ensure(id as usize);
        let i = id as usize;
        self.idx.name[i] = name.into();
        self.idx.parent[i] = parent;
        self.idx.size[i] = size;
        self.idx.mtime[i] = mtime;
        self.idx.flags[i] = FLAG_LIVE | if is_dir { FLAG_DIR } else { 0 };
    }

    /// Append a node with a sequential id (walkdir fallback).
    pub fn push(&mut self, parent: u32, name: &str, size: u64, mtime: i64, is_dir: bool) -> u32 {
        let id = self.idx.name.len() as u32;
        self.put(id, parent, name, size, mtime, is_dir);
        id
    }

    pub fn build(mut self, volume: impl Into<String>, root: u32) -> ScanIndex {
        self.idx.volume = volume.into();
        self.idx.root = root;
        // The root must exist and be marked live, or `finalize` drops the tree.
        self.ensure(root as usize);
        self.idx.flags[root as usize] |= FLAG_LIVE | FLAG_DIR;
        if self.idx.parent[root as usize] == u32::MAX {
            self.idx.parent[root as usize] = root;
        }
        self.idx.finalize();
        self.idx
    }
}

// ---------------------------------------------------------------------------
// Query surface
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFilter {
    /// Lowercase, no leading dot, e.g. `["png", "psd"]`.
    pub extensions: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    /// Unix seconds.
    pub modified_after: Option<i64>,
    pub modified_before: Option<i64>,
    /// Case-insensitive substring match against the full path.
    pub contains: Option<String>,
    /// Rust `regex` syntax, matched against the full path.
    pub path_regex: Option<String>,
    /// Limit results to this subtree.
    pub under: Option<u32>,
    pub include_dirs: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    #[default]
    Size,
    Name,
    Modified,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub id: u32,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: i64,
    pub is_dir: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub rows: Vec<FileRow>,
    pub total_matches: usize,
    pub total_bytes: u64,
}

impl ScanIndex {
    fn in_subtree(&self, mut node: u32, ancestor: u32) -> bool {
        let mut depth = 0;
        while depth < MAX_DEPTH {
            if node == ancestor {
                return true;
            }
            if node == self.root || (node as usize) >= self.len() {
                return false;
            }
            let p = self.parent[node as usize];
            if p == node {
                return false;
            }
            node = p;
            depth += 1;
        }
        false
    }

    pub fn query(
        &self,
        f: &FileFilter,
        sort: SortKey,
        desc: bool,
        offset: usize,
        limit: usize,
    ) -> QueryResult {
        let re = f
            .path_regex
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| regex::RegexBuilder::new(s).case_insensitive(true).build().ok());
        let needle = f.contains.as_deref().map(str::to_lowercase);
        let exts: Vec<String> = f
            .extensions
            .iter()
            .map(|e| e.trim_start_matches('.').to_lowercase())
            .filter(|e| !e.is_empty())
            .collect();

        let mut hits: Vec<u32> = Vec::new();
        let mut total_bytes = 0u64;

        for i in 0..self.len() {
            let id = i as u32;
            if !self.is_live(id) {
                continue;
            }
            let is_dir = self.is_dir(id);
            if is_dir && !f.include_dirs {
                continue;
            }
            let size = if is_dir { self.total[i] } else { self.size[i] };
            if let Some(m) = f.min_size {
                if size < m {
                    continue;
                }
            }
            if let Some(m) = f.max_size {
                if size > m {
                    continue;
                }
            }
            if let Some(t) = f.modified_after {
                if self.mtime[i] < t {
                    continue;
                }
            }
            if let Some(t) = f.modified_before {
                if self.mtime[i] > t {
                    continue;
                }
            }
            if !exts.is_empty() {
                let n = self.name[i].to_lowercase();
                let ok = n
                    .rsplit_once('.')
                    .is_some_and(|(_, e)| exts.iter().any(|x| x.as_str() == e));
                if !ok {
                    continue;
                }
            }
            if let Some(a) = f.under {
                if !self.in_subtree(id, a) {
                    continue;
                }
            }
            if needle.is_some() || re.is_some() {
                let p = self.path_of(id);
                if let Some(nd) = &needle {
                    if !p.to_lowercase().contains(nd.as_str()) {
                        continue;
                    }
                }
                if let Some(r) = &re {
                    if !r.is_match(&p) {
                        continue;
                    }
                }
            }
            total_bytes += size;
            hits.push(id);
        }

        let total_matches = hits.len();
        match sort {
            SortKey::Size => hits.sort_unstable_by_key(|&i| {
                let i = i as usize;
                std::cmp::Reverse(if self.flags[i] & FLAG_DIR != 0 {
                    self.total[i]
                } else {
                    self.size[i]
                })
            }),
            SortKey::Modified => hits.sort_unstable_by_key(|&i| std::cmp::Reverse(self.mtime[i as usize])),
            SortKey::Name => hits.sort_unstable_by(|&a, &b| {
                self.name[a as usize]
                    .to_lowercase()
                    .cmp(&self.name[b as usize].to_lowercase())
            }),
        }
        if desc == matches!(sort, SortKey::Name) {
            // Size/Modified sort descending by default; Name sorts ascending.
            hits.reverse();
        }

        let rows = hits
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|id| self.row(id))
            .collect();

        QueryResult {
            rows,
            total_matches,
            total_bytes,
        }
    }

    pub fn row(&self, id: u32) -> FileRow {
        let i = id as usize;
        let is_dir = self.is_dir(id);
        FileRow {
            id,
            path: self.path_of(id),
            name: self.name[i].to_string(),
            size: if is_dir { self.total[i] } else { self.size[i] },
            modified: self.mtime[i],
            is_dir,
        }
    }
}

// ---------------------------------------------------------------------------
// Treemap payload
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub child_count: usize,
    pub children: Vec<TreeNode>,
}

impl ScanIndex {
    /// Top-`fanout` children by size, recursed to `depth`. Everything below the
    /// cut is folded into a synthetic `<other>` node so the treemap's areas
    /// still add up to the parent's real size.
    pub fn tree(&self, node: u32, depth: u32, fanout: usize) -> TreeNode {
        let i = node as usize;
        let is_dir = self.is_dir(node);
        let size = if is_dir { self.total[i] } else { self.size[i] };

        let mut kids: Vec<TreeNode> = Vec::new();
        if is_dir && depth > 0 {
            let mut c: Vec<u32> = self.children(node).to_vec();
            c.sort_unstable_by_key(|&k| {
                let k = k as usize;
                std::cmp::Reverse(if self.flags[k] & FLAG_DIR != 0 {
                    self.total[k]
                } else {
                    self.size[k]
                })
            });
            let shown = c.len().min(fanout);
            let mut rest = 0u64;
            for &k in &c[shown..] {
                let k = k as usize;
                rest += if self.flags[k] & FLAG_DIR != 0 {
                    self.total[k]
                } else {
                    self.size[k]
                };
            }
            kids.extend(c[..shown].iter().map(|&k| self.tree(k, depth - 1, fanout)));
            if rest > 0 {
                kids.push(TreeNode {
                    id: u32::MAX,
                    name: format!("<{} more>", c.len() - shown),
                    path: String::new(),
                    size: rest,
                    is_dir: true,
                    child_count: c.len() - shown,
                    children: vec![],
                });
            }
        }

        TreeNode {
            id: node,
            name: if node == self.root && !self.volume.is_empty() {
                self.volume.clone()
            } else {
                self.name[i].to_string()
            },
            path: self.path_of(node),
            size,
            is_dir,
            child_count: self.children(node).len(),
            children: kids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ScanIndex {
        // root(0)
        //  ├ dir a(1)
        //  │   ├ f1(2) 100
        //  │   └ f2(3) 200
        //  └ f3(4) 50
        let mut b = IndexBuilder::with_capacity(8);
        b.put(0, 0, "", 0, 0, true);
        b.put(1, 0, "a", 0, 10, true);
        b.put(2, 1, "f1.txt", 100, 11, false);
        b.put(3, 1, "f2.bin", 200, 12, false);
        b.put(4, 0, "f3.txt", 50, 13, false);
        b.build("C:", 0)
    }

    #[test]
    fn rolls_subtree_totals_to_ancestors() {
        let ix = sample();
        assert_eq!(ix.total[0], 350);
        assert_eq!(ix.total[1], 300);
        assert_eq!(ix.file_count, 3);
        assert_eq!(ix.dir_count, 2);
    }

    #[test]
    fn reconstructs_windows_paths() {
        let ix = sample();
        assert_eq!(ix.path_of(2), r"C:\a\f1.txt");
        assert_eq!(ix.path_of(4), r"C:\f3.txt");
    }

    #[test]
    fn filters_by_extension_and_size() {
        let ix = sample();
        let f = FileFilter {
            extensions: vec!["txt".into()],
            min_size: Some(60),
            ..Default::default()
        };
        let r = ix.query(&f, SortKey::Size, true, 0, 100);
        assert_eq!(r.total_matches, 1);
        assert_eq!(r.rows[0].name, "f1.txt");
    }

    #[test]
    fn restricts_query_to_a_subtree() {
        let ix = sample();
        let f = FileFilter { under: Some(1), ..Default::default() };
        let r = ix.query(&f, SortKey::Size, true, 0, 100);
        assert_eq!(r.total_matches, 2);
        assert_eq!(r.total_bytes, 300);
    }

    #[test]
    fn treemap_folds_the_tail_into_an_other_node() {
        let ix = sample();
        let t = ix.tree(0, 2, 1);
        // 1 shown child + 1 "<n more>" bucket, and areas still sum to 350.
        assert_eq!(t.children.len(), 2);
        assert_eq!(t.children.iter().map(|c| c.size).sum::<u64>(), 350);
    }
}
