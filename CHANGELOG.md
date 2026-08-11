# Changelog

All notable changes to this project are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] — 2026-08-11

Initial release.

### Added
- Raw NTFS MFT scanner with boot-sector parsing, data-run decoding, update-sequence fixups and `$FILE_NAME`/`$DATA` extraction; parallel directory-walk fallback for non-NTFS, non-elevated and non-Windows cases.
- Struct-of-arrays scan index with CSR child lists, subtree size rollup, filtered querying (extension, size, date, substring, regex, subtree) and a squarified canvas treemap.
- Profile system covering Desktop, Documents, Opera GX, Chromium browsers, Firefox, Steam, git repos, Windows credentials, AI/editor tools, app configs and custom globs — persisted as user-editable JSON.
- Chromium password extraction via DPAPI (`v10`/`v11` and legacy blobs), with explicit detection of Chrome 127+ app-bound encryption and a guided manual-export path.
- Argon2id + AES-256-GCM envelope encryption with KDF parameters bound as associated data.
- Backup engine: plan, stage with inline hashing, seal secrets, archive (7z/AES-256 or zip/zstd), verify, manifest.
- Idempotent PowerShell restore script, self-contained HTML report and `unseal`/`verify`/`shred` CLI subcommands.
- Steam VDF parser, git repository and credential-helper discovery, Windows Credential Manager inventory.
