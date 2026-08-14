# Running a backup

## Plan first

Calculating a plan resolves every glob and stats every file while nothing is being written. The size you approve is the size you get. It also reports free space at the destination and warns when staging plus an archive would need more than you have — roughly twice the source size.

## Archive formats

| Format | Encrypted | Use when |
|---|---|---|
| **7z + AES-256** | Yes | Default. Smallest output, and the archive itself is protected. |
| **zip + zstd** | **No** | You want speed and the destination is already encrypted. |
| **Folder only** | n/a | You'll copy the staging folder yourself. |

Zip is honestly labelled unencrypted — legacy ZipCrypto is broken and pretending otherwise would be worse than not offering it. Sealed `.prb` artifacts inside stay sealed either way.

## The passphrase

Derives an Argon2id key (64 MiB, t=3, p=1) that seals every secret and, with 7z, the archive.

Minimum 12 characters and three character classes, or 20+ characters of anything. **There is no recovery.** Write it somewhere that isn't the machine you're about to wipe.

## What happens, in order

1. **Stage** — files are copied into `ReBackUp_<timestamp>/files/`, hashed during the copy so large data is read once, not twice.
2. **Seal** — secret actions run *after* staging, so a browser-profile failure can't strand half-copied files.
3. **Audit** — the run aborts if any unsealed file ended up under `secrets/`.
4. **Archive** — optional.
5. **Verify** — every staged file is re-hashed from disk.
6. **Manifest** — written last. Its presence means the run completed.

If verification fails, the report says *do not reset yet* in red. Believe it.

## Output

```
ReBackUp_20260811-174233/
├── files/C/Users/you/Desktop/...     mirrored source tree
├── secrets/*.rbu                     Argon2id + AES-256-GCM
├── manifest.json                     every path, size, SHA-256
├── report.html                       self-contained, opens offline
├── restore.ps1 / restore.cmd         idempotent restore driver
└── READ-ME-FIRST.txt                 for future you
```
