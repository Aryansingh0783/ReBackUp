//! Raw NTFS Master File Table reader — the WizTree-class fast path.
//!
//! # Why raw MFT instead of `FindFirstFile`/`walkdir`
//! `walkdir` costs one directory-open + N stat calls per directory and is
//! dominated by kernel round-trips. The MFT is a single, mostly-contiguous
//! system file that already contains every name, parent pointer, size and
//! timestamp on the volume. Reading it is one long sequential I/O, so a 1 TB
//! NVMe with ~2M files lands in a handful of seconds instead of minutes.
//!
//! # Requirements
//! Opening `\\.\C:` for raw reads requires membership in Administrators (or
//! `SeBackupPrivilege` + `SeRestorePrivilege`). We enable the backup privilege
//! if the token holds it, and surface `AccessDenied` otherwise so the caller
//! can fall back to [`super::walk`].
//!
//! # On-disk structures implemented here
//! * NTFS boot sector / BPB           (`$Boot`, LCN 0)
//! * MFT record header + fixup array  ("FILE" records)
//! * Attribute headers, resident + non-resident
//! * `$STANDARD_INFORMATION` (0x10), `$FILE_NAME` (0x30), `$DATA` (0x80)
//! * Data-run (run-list) decoding, including sparse runs
//!
//! # Known limitations (documented, not silently wrong)
//! * `$ATTRIBUTE_LIST` (0x20) is not followed. A file whose `$DATA` lives in an
//!   extension record reports its `$FILE_NAME` size instead — accurate to
//!   within one allocation for the pathological cases where this happens.
//! * Alternate data streams are ignored (named `$DATA` attributes are skipped);
//!   only the unnamed stream counts toward size.
//! * Compressed/sparse files report *logical* size, matching Explorer.

#![cfg(windows)]

use crate::error::{AppError, AppResult};
use crate::util::filetime_to_unix;

use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, OpenProcessToken,
    TokenElevation, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_ELEVATION, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, SetFilePointerEx, FILE_ATTRIBUTE_NORMAL, FILE_BEGIN, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::GetCurrentProcess;

/// `GENERIC_READ`. Hard-coded rather than imported because the constant has
/// moved between `windows` crate major versions.
const GENERIC_READ_U32: u32 = 0x8000_0000;

const ATTR_STANDARD_INFORMATION: u32 = 0x10;
const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;

/// MFT record 5 is always the volume root directory.
pub const ROOT_RECORD: u32 = 5;

/// Read this much of the MFT per I/O. Large enough to amortise syscalls,
/// small enough to keep peak RSS bounded on a 1 TB volume.
const CHUNK_BYTES: usize = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Raw volume handle
// ---------------------------------------------------------------------------

struct Volume {
    handle: HANDLE,
    bytes_per_sector: u32,
    cluster_size: u32,
    record_size: u32,
    mft_lcn: u64,
    total_clusters: u64,
}

impl Drop for Volume {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

impl Volume {
    /// `letter` is a bare drive letter such as `C`.
    fn open(letter: char) -> AppResult<Self> {
        // SeBackupPrivilege lets a member of Backup Operators read the raw
        // volume without full admin. Failure is non-fatal: the CreateFileW
        // below will tell us definitively whether we have access.
        unsafe { enable_privilege("SeBackupPrivilege") };

        // `\\.\C:` — the Win32 device path for the raw volume. Note: no
        // trailing backslash; that would name the *root directory* instead.
        let path: Vec<u16> = format!(r"\\.\{letter}:")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ_U32,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|e| {
            // 5 = ERROR_ACCESS_DENIED
            if e.code().0 as u32 & 0xFFFF == 5 {
                AppError::AccessDenied(format!(r"\\.\{letter}: (raw volume read)"))
            } else {
                AppError::Io(std::io::Error::other(format!(
                    "CreateFileW(\\\\.\\{letter}:) failed: {e}"
                )))
            }
        })?;

        let mut vol = Volume {
            handle,
            bytes_per_sector: 512,
            cluster_size: 4096,
            record_size: 1024,
            mft_lcn: 0,
            total_clusters: 0,
        };
        vol.read_boot_sector(letter)?;
        Ok(vol)
    }

    /// NTFS BPB. Offsets are from the NTFS on-disk spec and are stable since
    /// Windows NT 3.51.
    fn read_boot_sector(&mut self, letter: char) -> AppResult<()> {
        let mut boot = [0u8; 512];
        self.read_at(0, &mut boot)?;

        if &boot[3..11] != b"NTFS    " {
            return Err(AppError::NotNtfs(format!("{letter}:")));
        }

        let bytes_per_sector = u16::from_le_bytes([boot[0x0B], boot[0x0C]]) as u32;
        let sectors_per_cluster = boot[0x0D] as u32;
        if bytes_per_sector == 0 || sectors_per_cluster == 0 {
            return Err(AppError::Parse("zeroed BPB geometry".into()));
        }

        self.bytes_per_sector = bytes_per_sector;
        self.cluster_size = bytes_per_sector * sectors_per_cluster;
        self.total_clusters =
            u64::from_le_bytes(boot[0x28..0x30].try_into().unwrap()) / sectors_per_cluster as u64;
        self.mft_lcn = u64::from_le_bytes(boot[0x30..0x38].try_into().unwrap());

        // Signed byte: positive = clusters per record, negative = 2^|v| bytes.
        let cpr = boot[0x40] as i8;
        self.record_size = if cpr > 0 {
            cpr as u32 * self.cluster_size
        } else {
            1u32 << (-cpr) as u32
        };

        if self.record_size < 512 || self.record_size > 64 * 1024 {
            return Err(AppError::Parse(format!(
                "implausible MFT record size {}",
                self.record_size
            )));
        }
        Ok(())
    }

    /// Raw volume reads must be sector-aligned in both offset and length. All
    /// callers here work in whole clusters, which satisfies that by
    /// construction; the assertion catches future regressions.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> AppResult<()> {
        debug_assert_eq!(offset % self.bytes_per_sector as u64, 0);
        debug_assert_eq!(buf.len() % self.bytes_per_sector as usize, 0);

        unsafe {
            SetFilePointerEx(self.handle, offset as i64, None, FILE_BEGIN)
                .map_err(|e| AppError::Io(std::io::Error::other(format!("seek: {e}"))))?;
        }

        let mut done = 0usize;
        while done < buf.len() {
            let mut read = 0u32;
            unsafe {
                ReadFile(self.handle, Some(&mut buf[done..]), Some(&mut read), None)
                    .map_err(|e| AppError::Io(std::io::Error::other(format!("read: {e}"))))?;
            }
            if read == 0 {
                return Err(AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "short read from raw volume",
                )));
            }
            done += read as usize;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Privilege helpers
// ---------------------------------------------------------------------------

unsafe fn enable_privilege(name: &str) -> bool {
    let mut token = HANDLE::default();
    if OpenProcessToken(
        GetCurrentProcess(),
        TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
        &mut token,
    )
    .is_err()
    {
        return false;
    }
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let mut luid = LUID::default();
    let ok = LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(wide.as_ptr()), &mut luid).is_ok();
    let mut result = false;
    if ok {
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // NOTE: AdjustTokenPrivileges reports success even when the privilege
        // was not held (GetLastError == ERROR_NOT_ALL_ASSIGNED). We deliberately
        // don't check that — CreateFileW is the real authority on access.
        result = AdjustTokenPrivileges(
            token,
            false.into(),
            Some(&tp as *const TOKEN_PRIVILEGES),
            0,
            None,
            None,
        )
        .is_ok();
    }
    let _ = CloseHandle(token);
    result
}

/// True when the current process runs with an elevated token. The UI uses this
/// to decide whether to offer the MFT fast path or the fallback up front.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

// ---------------------------------------------------------------------------
// Record parsing
// ---------------------------------------------------------------------------

/// One usable MFT entry, flattened to exactly what the scanner index needs.
pub struct RawEntry {
    pub record: u32,
    pub parent: u32,
    pub name: String,
    pub size: u64,
    pub allocated: u64,
    pub mtime: i64,
    pub is_dir: bool,
}

/// Undo the update-sequence "fixups" NTFS writes into the last two bytes of
/// every sector so torn writes are detectable. Must run before any field past
/// the first sector is trusted.
fn apply_fixups(rec: &mut [u8], bytes_per_sector: u32) -> Result<(), &'static str> {
    let usa_off = u16::from_le_bytes([rec[0x04], rec[0x05]]) as usize;
    let usa_cnt = u16::from_le_bytes([rec[0x06], rec[0x07]]) as usize;
    if usa_cnt == 0 || usa_off + usa_cnt * 2 > rec.len() {
        return Err("bad update sequence array bounds");
    }
    let usn = [rec[usa_off], rec[usa_off + 1]];
    for i in 1..usa_cnt {
        let sector_end = i * bytes_per_sector as usize - 2;
        if sector_end + 2 > rec.len() {
            return Err("update sequence array overruns record");
        }
        if rec[sector_end] != usn[0] || rec[sector_end + 1] != usn[1] {
            return Err("update sequence mismatch (torn write / not a valid record)");
        }
        rec[sector_end] = rec[usa_off + i * 2];
        rec[sector_end + 1] = rec[usa_off + i * 2 + 1];
    }
    Ok(())
}

struct RunEntry {
    lcn: Option<u64>, // None = sparse
    clusters: u64,
}

/// Decode an NTFS data-run list. Each run is `[hdr][length][offset]` where the
/// header nibbles give the byte-widths of the two fields and the offset is a
/// *signed delta* from the previous run's LCN.
fn decode_runs(buf: &[u8]) -> Vec<RunEntry> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    let mut i = 0usize;

    while i < buf.len() {
        let hdr = buf[i];
        if hdr == 0 {
            break;
        }
        let len_sz = (hdr & 0x0F) as usize;
        let off_sz = (hdr >> 4) as usize;
        i += 1;
        if len_sz == 0 || len_sz > 8 || off_sz > 8 || i + len_sz + off_sz > buf.len() {
            break;
        }

        let mut clusters: u64 = 0;
        for j in 0..len_sz {
            clusters |= (buf[i + j] as u64) << (8 * j);
        }
        i += len_sz;

        if off_sz == 0 {
            runs.push(RunEntry { lcn: None, clusters });
            continue;
        }

        let mut delta: i64 = 0;
        for j in 0..off_sz {
            delta |= (buf[i + j] as i64) << (8 * j);
        }
        // Sign-extend from off_sz bytes to 64 bits.
        let shift = 64 - 8 * off_sz as u32;
        delta = (delta << shift) >> shift;
        i += off_sz;

        lcn += delta;
        if lcn < 0 {
            break;
        }
        runs.push(RunEntry {
            lcn: Some(lcn as u64),
            clusters,
        });
    }
    runs
}

/// Walk the attributes of one (already fixed-up) record.
fn parse_record(rec: &[u8], record_no: u32) -> Option<RawEntry> {
    if &rec[0..4] != b"FILE" {
        return None;
    }
    let flags = u16::from_le_bytes([rec[0x16], rec[0x17]]);
    let in_use = flags & 0x01 != 0;
    if !in_use {
        return None;
    }
    let is_dir = flags & 0x02 != 0;

    // Extension records point back at a base record; the base carries the name.
    let base_ref = u64::from_le_bytes(rec[0x20..0x28].try_into().ok()?);
    if base_ref & 0x0000_FFFF_FFFF_FFFF != 0 {
        return None;
    }

    let mut off = u16::from_le_bytes([rec[0x14], rec[0x15]]) as usize;
    let used = u32::from_le_bytes(rec[0x18..0x1C].try_into().ok()?) as usize;
    let limit = used.min(rec.len());

    let mut best_name: Option<(u8, String)> = None; // (namespace, name)
    let mut parent: u32 = u32::MAX;
    let mut data_size: Option<u64> = None;
    let mut data_alloc: Option<u64> = None;
    let mut fn_size: u64 = 0;
    let mut mtime: i64 = 0;
    let mut saw_attribute_list = false;

    while off + 8 <= limit {
        let atype = u32::from_le_bytes(rec[off..off + 4].try_into().ok()?);
        if atype == ATTR_END {
            break;
        }
        let alen = u32::from_le_bytes(rec[off + 4..off + 8].try_into().ok()?) as usize;
        if alen < 16 || off + alen > limit {
            break;
        }
        let non_resident = rec[off + 8] != 0;
        let name_len = rec[off + 9] as usize;

        match atype {
            ATTR_STANDARD_INFORMATION if !non_resident => {
                let voff = u16::from_le_bytes([rec[off + 0x14], rec[off + 0x15]]) as usize;
                let v = off + voff;
                if v + 24 <= limit {
                    mtime = filetime_to_unix(u64::from_le_bytes(
                        rec[v + 8..v + 16].try_into().ok()?,
                    ));
                }
            }
            ATTR_ATTRIBUTE_LIST => saw_attribute_list = true,
            ATTR_FILE_NAME if !non_resident => {
                let voff = u16::from_le_bytes([rec[off + 0x14], rec[off + 0x15]]) as usize;
                let v = off + voff;
                if v + 0x42 <= limit {
                    let pref = u64::from_le_bytes(rec[v..v + 8].try_into().ok()?);
                    let nlen = rec[v + 0x40] as usize;
                    let namespace = rec[v + 0x41];
                    let nstart = v + 0x42;
                    if nstart + nlen * 2 <= limit {
                        let units: Vec<u16> = (0..nlen)
                            .map(|k| u16::from_le_bytes([rec[nstart + k * 2], rec[nstart + k * 2 + 1]]))
                            .collect();
                        let name = String::from_utf16_lossy(&units);
                        // Namespace preference: Win32&DOS(3) > Win32(1) > POSIX(0) > DOS(2).
                        let rank = |ns: u8| match ns {
                            3 => 3u8,
                            1 => 2,
                            0 => 1,
                            _ => 0,
                        };
                        if best_name
                            .as_ref()
                            .map_or(true, |(cur, _)| rank(namespace) > rank(*cur))
                        {
                            best_name = Some((namespace, name));
                            parent = (pref & 0x0000_FFFF_FFFF_FFFF) as u32;
                        }
                        fn_size = fn_size.max(u64::from_le_bytes(
                            rec[v + 0x30..v + 0x38].try_into().ok()?,
                        ));
                    }
                }
            }
            ATTR_DATA if name_len == 0 => {
                // Unnamed $DATA only — named streams are ADS and don't count.
                if non_resident {
                    let start_vcn = u64::from_le_bytes(rec[off + 0x10..off + 0x18].try_into().ok()?);
                    // Real size lives only in the *first* extent's header.
                    if start_vcn == 0 && off + 0x38 <= limit {
                        data_alloc =
                            Some(u64::from_le_bytes(rec[off + 0x28..off + 0x30].try_into().ok()?));
                        data_size =
                            Some(u64::from_le_bytes(rec[off + 0x30..off + 0x38].try_into().ok()?));
                    }
                } else {
                    let vlen = u32::from_le_bytes(rec[off + 0x10..off + 0x14].try_into().ok()?);
                    data_size = Some(vlen as u64);
                    data_alloc = Some(vlen as u64);
                }
            }
            _ => {}
        }
        off += alen;
    }

    let (_, name) = best_name?;
    if parent == u32::MAX {
        return None;
    }

    // Fall back to $FILE_NAME's size when $DATA lived in an extension record.
    let size = if is_dir {
        0
    } else {
        data_size.unwrap_or(if saw_attribute_list { fn_size } else { 0 })
    };

    Some(RawEntry {
        record: record_no,
        parent,
        name,
        size,
        allocated: if is_dir { 0 } else { data_alloc.unwrap_or(size) },
        mtime,
        is_dir,
    })
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Enumerate every in-use record on `letter:`, invoking `sink` per entry and
/// `progress(done_records, total_records)` roughly 200 times over the scan.
///
/// `cancel` is polled per chunk; returning `true` aborts and yields `Ok(false)`.
pub fn enumerate<F, P, C>(
    letter: char,
    mut sink: F,
    mut progress: P,
    mut cancel: C,
) -> AppResult<bool>
where
    F: FnMut(RawEntry),
    P: FnMut(u64, u64),
    C: FnMut() -> bool,
{
    let vol = Volume::open(letter)?;

    // --- Step 1: read record 0 ($MFT) and decode its own $DATA run list. -----
    let rec_size = vol.record_size as usize;
    let mut rec0 = vec![0u8; rec_size.max(vol.cluster_size as usize)];
    vol.read_at(vol.mft_lcn * vol.cluster_size as u64, &mut rec0)?;
    apply_fixups(&mut rec0[..rec_size], vol.bytes_per_sector)
        .map_err(|e| AppError::Parse(format!("$MFT record 0: {e}")))?;

    let runs = extract_mft_runs(&rec0[..rec_size])
        .ok_or_else(|| AppError::Parse("could not locate $MFT $DATA run list".into()))?;

    let total_clusters: u64 = runs.iter().map(|r| r.clusters).sum();
    let total_records = total_clusters * vol.cluster_size as u64 / rec_size as u64;
    let mut done_records: u64 = 0;
    let progress_every = (total_records / 200).max(1);
    let mut next_progress = progress_every;

    // --- Step 2: stream the MFT run by run. ---------------------------------
    let chunk_clusters = (CHUNK_BYTES as u64 / vol.cluster_size as u64).max(1);
    let mut buf = vec![0u8; (chunk_clusters * vol.cluster_size as u64) as usize];
    let mut record_no: u64 = 0;

    for run in &runs {
        let Some(lcn) = run.lcn else {
            // Sparse hole inside $MFT: no records, just advance the counter.
            record_no += run.clusters * vol.cluster_size as u64 / rec_size as u64;
            continue;
        };

        let mut remaining = run.clusters;
        let mut cluster = 0u64;
        while remaining > 0 {
            if cancel() {
                return Ok(false);
            }
            let take = remaining.min(chunk_clusters);
            let bytes = (take * vol.cluster_size as u64) as usize;
            vol.read_at((lcn + cluster) * vol.cluster_size as u64, &mut buf[..bytes])?;

            for chunk in buf[..bytes].chunks_mut(rec_size) {
                let this_record = record_no as u32;
                record_no += 1;
                done_records += 1;

                if &chunk[0..4] != b"FILE" {
                    continue; // free / never-allocated record
                }
                if apply_fixups(chunk, vol.bytes_per_sector).is_err() {
                    continue;
                }
                if let Some(entry) = parse_record(chunk, this_record) {
                    sink(entry);
                }
            }

            if done_records >= next_progress {
                progress(done_records, total_records);
                next_progress = done_records + progress_every;
            }

            remaining -= take;
            cluster += take;
        }
    }

    progress(total_records, total_records);
    Ok(true)
}

/// Pull the unnamed non-resident `$DATA` run list out of MFT record 0.
fn extract_mft_runs(rec: &[u8]) -> Option<Vec<RunEntry>> {
    let mut off = u16::from_le_bytes([rec[0x14], rec[0x15]]) as usize;
    let used = u32::from_le_bytes(rec[0x18..0x1C].try_into().ok()?) as usize;
    let limit = used.min(rec.len());

    while off + 8 <= limit {
        let atype = u32::from_le_bytes(rec[off..off + 4].try_into().ok()?);
        if atype == ATTR_END {
            break;
        }
        let alen = u32::from_le_bytes(rec[off + 4..off + 8].try_into().ok()?) as usize;
        if alen < 16 || off + alen > limit {
            break;
        }
        let non_resident = rec[off + 8] != 0;
        let name_len = rec[off + 9] as usize;

        if atype == ATTR_DATA && non_resident && name_len == 0 {
            let run_off = u16::from_le_bytes([rec[off + 0x20], rec[off + 0x21]]) as usize;
            if off + run_off < off + alen {
                return Some(decode_runs(&rec[off + run_off..off + alen]));
            }
        }
        off += alen;
    }
    None
}

/// Total/free bytes for a volume, used by the drive picker.
pub fn volume_geometry(letter: char) -> AppResult<(u64, u32)> {
    let vol = Volume::open(letter)?;
    Ok((vol.total_clusters * vol.cluster_size as u64, vol.cluster_size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_simple_run_list() {
        // 0x21 => 1-byte length, 2-byte offset. len=0x18, delta=0x0C34.
        // 0x11 => 1-byte length, 1-byte offset. len=0x08, delta=+0x10.
        let bytes = [0x21, 0x18, 0x34, 0x0C, 0x11, 0x08, 0x10, 0x00];
        let runs = decode_runs(&bytes);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].clusters, 0x18);
        assert_eq!(runs[0].lcn, Some(0x0C34));
        assert_eq!(runs[1].clusters, 0x08);
        assert_eq!(runs[1].lcn, Some(0x0C34 + 0x10));
    }

    #[test]
    fn sign_extends_negative_run_offsets() {
        // Second run steps *backwards* by 0x10 (0xF0 as a signed byte = -16).
        let bytes = [0x11, 0x04, 0x40, 0x11, 0x04, 0xF0, 0x00];
        let runs = decode_runs(&bytes);
        assert_eq!(runs[0].lcn, Some(0x40));
        assert_eq!(runs[1].lcn, Some(0x30));
    }

    #[test]
    fn handles_sparse_runs() {
        // 0x01 => length only, no offset => sparse hole.
        let bytes = [0x11, 0x02, 0x20, 0x01, 0x05, 0x00];
        let runs = decode_runs(&bytes);
        assert_eq!(runs.len(), 2);
        assert!(runs[1].lcn.is_none());
        assert_eq!(runs[1].clusters, 5);
    }

    #[test]
    fn rejects_torn_records() {
        let mut rec = vec![0u8; 1024];
        rec[..4].copy_from_slice(b"FILE");
        rec[0x04] = 0x30; // usa offset
        rec[0x06] = 3; // usa count: usn + 2 sectors
        rec[0x30] = 0xAA;
        rec[0x31] = 0xBB;
        rec[510] = 0x00; // deliberately wrong
        rec[511] = 0x00;
        assert!(apply_fixups(&mut rec, 512).is_err());
    }
}
