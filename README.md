# Pre-Reset Backup

**Scan your drives like WizTree, pick what actually matters, and walk away with a verified, encrypted backup — before you wipe Windows.**

A clean install is easy. Remembering what you had is not. This app finds the things that only exist on this machine — desktop files, Opera GX passwords, Steam sessions, git repos with unpushed work, editor and AI-tool config, Windows credentials — copies them, hashes them, encrypts the secret parts, and hands you a script that puts it all back.

Local-only by design. Nothing is uploaded anywhere, ever.

```
┌──────────┐   ┌───────────┐   ┌──────────┐   ┌──────────┐   ┌────────┐
│  Scan    │──▶│  Choose   │──▶│  Review  │──▶│   Run    │──▶│ Verify │
│ MFT/walk │   │ profiles  │   │ + size   │   │ seal/zip │   │ + copy │
└──────────┘   └───────────┘   └──────────┘   └──────────┘   └────────┘
```

---

## Quick start

```bash
git clone https://github.com/YOURNAME/pre-reset-backup
cd pre-reset-backup
pnpm install
pnpm tauri dev          # development
pnpm tauri build        # produces .msi + NSIS .exe in src-tauri/target/release/bundle
```

Prerequisites: Node 20+, pnpm 9+, Rust 1.82+, and on Windows the [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (already present on Windows 10 21H2+ and Windows 11).

**Run it elevated.** Reading the NTFS Master File Table needs administrator rights. Without them the app still works — it silently falls back to a directory walk that's 10–40× slower on a large volume.

---

## What it backs up, and how

| Thing | What actually happens | Comes back? |
|---|---|---|
| **Desktop / Documents** | Straight file copy with SHA-256 per file | Yes |
| **Opera GX passwords** | `Local State` → DPAPI → AES-256-GCM key → decrypt `Login Data` → re-seal as CSV with *your* passphrase | Yes, via CSV import |
| **Opera GX profile** | Copies both `%APPDATA%` and `%LOCALAPPDATA%` trees, minus caches | Tabs, bookmarks, extensions, sessions |
| **Chrome / Edge / Brave / Vivaldi** | Same DPAPI path, all profiles including `Profile 1..n` | Yes |
| **Firefox** | Whole-profile copy (`logins.json` + `key4.db` travel together) | Yes, unless you use a Primary Password |
| **Steam** | `loginusers.vdf`, `config.vdf`, `local.vdf`, `ssfn*`, `userdata/<id3>/` | Account list and settings — **expect to re-enter a Steam Guard code** |
| **Git repos** | Finds every `.git` on scanned drives, records remotes/branch/dirty state, flags repos with work that exists nowhere else | Yes |
| **Git & SSH credentials** | `~/.git-credentials` and private keys are *sealed*, never copied in the clear | Yes (and you should rotate them) |
| **Windows Credentials** | Full inventory via `CredEnumerateW`; guided `.crd` export through the secure-desktop wizard | Inventory always; secrets only if you run the wizard |
| **AI & editor tools** | VS Code, Cursor, Windsurf, Claude Desktop, ChatGPT, JetBrains, Continue, Zed, Ollama, LM Studio | Settings yes; model weights deliberately excluded |

Anything not on this list is a **Custom** profile away — glob patterns with `%ENV%` expansion.

---

## Security model

### What is protected, and how

```
 Opera/Chrome                        your passphrase
 "Local State"                              │
      │  base64 → strip "DPAPI"             ▼
      ▼                              ┌─────────────┐
 CryptUnprotectData ── 32-byte AES key│  Argon2id   │ m=64MiB t=3 p=1
      │                              │  16B salt   │
      ▼                              └──────┬──────┘
 "Login Data" (SQLite)                      │ 32-byte key
   password_value = v10 ‖ nonce ‖ GCM       ▼
      │                            ┌──────────────────┐
      ▼  AES-256-GCM decrypt       │  AES-256-GCM     │
  plaintext ───── in memory only ─▶│  KDF params      │──▶ *.prb on disk
  (Zeroizing, wiped on drop)       │  bound as AAD    │
                                   └──────────────────┘
```

Concretely:

- **Never on disk in the clear.** Decrypted passwords exist only inside `Zeroizing` containers, are assembled into a CSV in memory, sealed, and dropped. There is no temp file to forget about.
- **Argon2id, 64 MiB / t=3 / p=1** for key derivation. Salt and nonce are random per blob.
- **AES-256-GCM** with the KDF parameters bound in as associated data — an attacker can't edit `m_cost` down to 8 KiB and have the blob still authenticate. There's a test for exactly that.
- **The manifest records `encrypted: true`, the algorithm and the KDF — never a plaintext, never a hash of a secret.** Hashing a short password just moves the problem.
- **A guard rail refuses to finish** a backup if any unsealed file ends up under `secrets/`.
- **Wrong passphrase and tampered ciphertext produce the same error.** Deliberately.
- **`Login Data` is never opened in place.** It's copied (with its WAL), read read-only, then the copy is overwritten and unlinked — writing to the live DB would corrupt the user's profile.

### What is *not* protected

Be clear-eyed about this:

- **The staging folder is plaintext for everything that isn't a secret.** Your documents are documents. Encrypt the archive (7z + AES-256) or the drive if that matters.
- **Shredding is best-effort on an SSD.** Wear levelling may have already relocated the original blocks. The tool says so when you run it.
- **Losing the passphrase means losing the sealed data.** No recovery, no backdoor, no key escrow.
- **DPAPI is scoped to your logon session.** This is why the tool works for *you on this machine* and cannot decrypt another account's profile, or one from an offline disk. That's a property worth keeping, not a bug to route around.
- **App-bound encryption (Chrome 127+ and equivalents) is not defeated.** Those keys are sealed to a SYSTEM-level service. The app detects it, says so plainly, and routes you to the browser's own export instead of failing mysteriously.
- **`ssfn*` sentry files are bound to machine + user SID.** After a clean install your SID changes. Back them up anyway — they cost nothing — but plan on re-authenticating.

### Attacker model

| Attacker | Outcome |
|---|---|
| Finds your backup drive | Sees your files; sealed artifacts need the passphrase (Argon2id at 64 MiB makes bulk cracking expensive) |
| Malware already running as you | Already has DPAPI access — this app changes nothing about that |
| Another user on the same PC | Cannot decrypt your DPAPI blobs, cannot read another account's profile |
| Someone with your old disk, offline | Cannot use DPAPI at all; sealed blobs still need the passphrase |

---

## Repository layout

```
pre-reset-backup/
├── src-tauri/
│   ├── src/
│   │   ├── scanner/
│   │   │   ├── mft.rs       raw NTFS MFT reader — boot sector, run-lists,
│   │   │   │                fixups, $FILE_NAME/$DATA. The fast path.
│   │   │   ├── walk.rs      parallel directory walk (portable fallback)
│   │   │   ├── index.rs     struct-of-arrays index + CSR children + treemap
│   │   │   └── mod.rs       backend selection, pause/resume/cancel, events
│   │   ├── secrets/
│   │   │   ├── dpapi.rs     CryptUnprotectData / CryptProtectData
│   │   │   ├── chromium.rs  Local State key + Login Data decryption
│   │   │   ├── vault.rs     Credential Manager inventory + guided export
│   │   │   └── mod.rs       sealing, shredding, invariants
│   │   ├── crypto.rs        Argon2id + AES-256-GCM envelope
│   │   ├── profiles.rs      declarative "what to take"
│   │   ├── detect.rs        installed-tool detection
│   │   ├── backup.rs        plan → stage → seal → archive → verify
│   │   ├── manifest.rs      manifest + verification
│   │   ├── restore.rs       restore.ps1 generation
│   │   ├── report.rs        self-contained HTML report
│   │   ├── steam.rs         Steam discovery
│   │   ├── git.rs           repo + credential-helper discovery
│   │   ├── opera.rs         browser profile detection
│   │   ├── vdf.rs           Valve KeyValues parser
│   │   ├── cli.rs           unseal / verify / shred subcommands
│   │   └── lib.rs           every Tauri command
│   ├── assets/restore.ps1.tmpl
│   ├── capabilities/default.json    least-privilege permission set
│   └── tauri.conf.json
├── src/                     React 19 + TypeScript + Tailwind
│   ├── components/          TreemapView (canvas), FileTable (virtualised), …
│   ├── stores/              zustand
│   └── lib/api.ts           typed command wrappers
└── docs/                    VitePress site
```

---

## Restoring

The backup folder is self-describing. On the fresh install:

```powershell
# 1. Preview
.\restore.ps1 -DryRun

# 2. Restore (idempotent — safe to re-run after a partial failure)
.\restore.cmd

# 3. Only some profiles
.\restore.ps1 -Only desktop,ai-tools

# 4. Unseal the password CSV, import it, then destroy it
pre-reset-backup.exe unseal --in secrets\opera-gx-default-passwords.csv.prb --out passwords.csv
#   → browser → password manager → Import → passwords.csv
pre-reset-backup.exe shred --path passwords.csv
```

`restore.ps1` verifies every file's SHA-256 before writing it and skips files that are already correct, so re-running costs almost nothing.

Check a backup any time — ideally *from the copy on the external drive*, which also proves the copy worked:

```powershell
pre-reset-backup.exe verify --manifest E:\PreResetBackup_20260811-174233\manifest.json
```

---

## Performance

The MFT reader does one long sequential read of `$MFT` and parses records in 8 MiB chunks, rather than issuing a directory-open plus a stat per file. Index construction is a struct-of-arrays keyed by MFT record number, so parent lookups are a single array index and a 3M-file volume costs roughly 200 MB of RAM instead of the ~1 GB a naive `Vec<PathBuf>` would need.

Measure it on your own hardware — `Scanner → summary line` reports elapsed time and which backend ran. If it says `walk`, you weren't elevated.

---

## Deviations from the original specification

Places where the spec asked for one thing and this repo does another, with reasons. Each was a deliberate call, not an oversight.

| Spec said | This does | Why |
|---|---|---|
| `dpapi-core` crate | Direct `CryptUnprotectData` via `windows` | `dpapi-core` implements **DPAPI-NG** (MS-GKDI group keys for LAPS/AD), a different protocol from the per-user DPAPI that Chromium uses. The Win32 call is ~40 lines with no extra supply-chain surface. |
| `ntfs-mft` crate | Hand-written MFT parser | Keeps the parse loop, the fixup handling and the run-list decoder auditable and unit-tested, with no dependency on a thinly-maintained crate for the single most correctness-critical path in the app. |
| Drive Opera's Export button over `--remote-debugging-port` | Direct DPAPI decryption; guided manual export as fallback | Opening a debug port lets **any** local process drive the browser and read its cookies for as long as it's open. Trading a real security hole for UI convenience is a bad deal when direct decryption gets the same data. |
| `vaultcmd /backup` | `rundll32 keymgr.dll,KRShowKeyMgr` + guidance | `vaultcmd` has no `/backup` verb on modern Windows. The wizard is the supported path, and it *requires* the secure desktop precisely so automation can't drive it. We inventory credentials automatically and guide the export. |
| Rust 2024 edition | Rust 2021, MSRV 1.82 | 2024 needs rustc ≥ 1.85 and this crate uses nothing from it. Lower MSRV, more contributors. |
| `recharts` treemap | Canvas squarified treemap | 2000+ SVG nodes with hover handlers janks the window. Canvas plus a hit-test array holds 60fps, and squarification (Bruls/Huizing/van Wijk) keeps rectangles comparable — the thing naive slice-and-dice gets wrong. |
| ZIP as the light archive | zip + zstd, **explicitly unencrypted** | Legacy ZipCrypto is broken. We don't label something encrypted when it isn't; the sealed artifacts inside stay sealed regardless. |
| `tauri-plugin-fs` + `tauri-plugin-shell` enabled | Neither plugin is loaded | Those plugins grant the **webview** filesystem and process access. Every file operation here already runs inside a purpose-built Rust command, so enabling them would widen the attack surface and buy nothing. The capability file grants only `dialog` and `opener`. |
| — | `.prb` sealed-blob format + CLI `unseal` | The restore script can't run Argon2id, so the app doubles as a small CLI. One binary, no sidecar. |

---

## Status against the original acceptance criteria

| Criterion | Status |
|---|---|
| Scans 1 TB NVMe in < 10 s (MFT path) | Implemented; **measure on your hardware** — no benchmark is claimed here that wasn't run |
| Opera GX passwords → restorable CSV | Implemented (v10/v11 + legacy DPAPI); app-bound (v20) profiles route to guided export |
| Steam `ssfn*` + config backed up | Implemented, with the SID limitation documented rather than papered over |
| Discovers `.git` repos, exports credential-helper config | Implemented, free from an existing scan index |
| Encrypted 7z | Implemented behind the `sevenz` feature (default on) |
| Restore script works on clean Windows 11 24H2 | Script is written and idempotent; **needs a VM run to be called verified** |
| Bundle < 15 MB | Tauri + `opt-level="s"` + LTO + strip; verify with `pnpm tauri build` |
| Zero plaintext secrets in any artifact | Enforced by the manifest audit, the sealing invariants, and unit tests |

Unverified rows are marked unverified on purpose. Claiming a benchmark nobody ran is how you lose a user's data and their trust at the same time.

---

## Non-goals

- Cloud sync. Local-only, deliberately.
- Full-disk imaging — use Macrium Reflect or Clonezilla.
- Decrypting another account's DPAPI blobs or an offline disk. Not a limitation to work around.
- Browsers with no export or decryption path.

---

## Development

```bash
pnpm dev                  # Vite only
pnpm tauri dev            # full app
pnpm typecheck && pnpm lint
cd src-tauri && cargo test --all-features
cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings
PRB_LOG=debug pnpm tauri dev     # verbose backend logging
```

Feature flags (`src-tauri/Cargo.toml`):

- `sevenz` *(default)* — 7z/LZMA2 + AES-256 archives.
- `mft` — reserved for forcing the raw-MFT path in builds that want it unconditionally.

See [CONTRIBUTING.md](CONTRIBUTING.md) for the rules around anything that touches secret material, and [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

MIT — see [LICENSE](LICENSE).
