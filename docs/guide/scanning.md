# Scanning drives

## Two backends

**MFT** (Windows + NTFS + elevated) reads `$MFT` directly. The Master File Table already contains every name, parent pointer, size and timestamp on the volume, so enumerating it is one long sequential read rather than millions of kernel round-trips.

**Walk** (everything else) uses a parallel directory traversal. No privileges needed; works on ReFS, exFAT, FAT32, ext4 and APFS.

`Auto` tries MFT and falls back silently, telling you why in a banner. If you see that banner, you weren't elevated.

## The treemap

Rectangle area is proportional to size, colour is derived from file extension, and clicking a directory drills in. Breadcrumbs walk back out. Only the top 24 children of any directory are drawn individually — the rest collapse into a `<n more>` block that still occupies its true area, so proportions never lie.

Layout is squarified (Bruls/Huizing/van Wijk), which keeps rectangles near-square. Naive slice-and-dice produces slivers you can't visually compare, which defeats the point.

## Filters

| Filter | Notes |
|---|---|
| `path contains` | case-insensitive substring |
| `ext` | comma-separated, with or without dots |
| `min MB` | useful set to 100 to find what actually matters |
| `regex` | Rust regex syntax, case-insensitive, matched against the full path |

Filters compose. `regex: \.(kdbx\|env\|pem)$` finds password databases, environment files and keys in one pass — a good sanity check before any reset.

Selections made here ride along with the backup as hand-picked extras, on top of whatever the profiles match.

## Pause, resume, cancel

Every backend polls a shared control flag between chunks. Pausing a scan blocks the worker without unwinding, so resuming costs nothing. Cancelling returns partial results rather than pretending the scan finished.
