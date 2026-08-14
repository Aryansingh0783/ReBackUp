<div align="center">

# ReBackUp

**Scan your drives like WizTree, pick what actually matters, and walk away with a verified, encrypted backup — before you wipe Windows.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tauri 2](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-1.82%2B-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![React 19](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)](https://react.dev)
[![Platform](https://img.shields.io/badge/Windows-first-0078D4?logo=windows&logoColor=white)](#)
[![Status](https://img.shields.io/badge/status-v0.0.0%20early-orange)](#project-status)

</div>

---

## The problem

Reinstalling Windows is easy. Remembering what you had is not.

You back up your Documents folder, wipe the disk, and three days later you discover the things you *actually* lost: the saved passwords in Opera GX, the half-finished repo that was never pushed, six Steam accounts you now have to re-authenticate, the VS Code settings you spent a year tuning, an SSH key, a `.env` file, the API token in `.npmrc`.

None of that lives in Documents. It's scattered across `%APPDATA%`, `%LOCALAPPDATA%`, Program Files and the Windows Credential vault — and most of it is encrypted to a Windows account that is about to stop existing.

**ReBackUp finds those things, copies them, verifies them, encrypts the sensitive parts, and gives you a script that puts it all back.**

Everything is local. Nothing is uploaded anywhere, ever. There is no account, no telemetry, and no network code in the app at all.

---

## What it backs up

| Thing | What actually happens | Comes back after the reset? |
|---|---|---|
| **Desktop / Documents** | File copy with a SHA-256 recorded per file | Yes |
| **Opera GX passwords** | `Local State` → DPAPI → AES-256-GCM key → decrypts `Login Data` → re-sealed as a CSV under **your** passphrase | Yes, via CSV import |
| **Opera GX profile** | Both the `%APPDATA%` and `%LOCALAPPDATA%` trees, minus caches | Tabs, bookmarks, extensions, sessions |
| **Chrome / Edge / Brave / Vivaldi** | Same DPAPI path, every profile including `Profile 1..n` | Yes |
| **Firefox** | Whole-profile copy — `logins.json` and `key4.db` travel together | Yes, unless you set a Primary Password you've forgotten |
| **Steam** | `loginusers.vdf`, `config.vdf`, `local.vdf`, `ssfn*`, `userdata/<id3>/` | Account list and per-game settings — **expect a Steam Guard code** |
| **Git repos** | Finds every `.git` on scanned drives, records remotes / branch / dirty state, flags repos whose work exists nowhere else | Yes |
| **Git & SSH credentials** | `~/.git-credentials` and private keys are *sealed*, never copied in the clear | Yes — and you should rotate them |
| **Windows Credentials** | Full inventory via `CredEnumerateW`, plus a guided `.crd` export | Inventory always; secrets if you run the wizard |
| **AI & editor tools** | VS Code, Cursor, Windsurf, Claude Desktop, ChatGPT, JetBrains, Continue, Zed, Ollama, LM Studio | Settings yes; model weights deliberately excluded |

Anything not on this list is one **Custom** profile away — glob patterns with `%ENV%` expansion.

---

## Screenshots

> Not included in the repo yet: the only screenshots taken so far are of a real machine and contain real filesystem paths. Run `pnpm tauri dev` and grab your own, or open an issue if you'd like anonymised ones added.

The UI is five steps, left to right:

```
Overview  →  Scanner  →  What to keep  →  Review & run  →  Result
   │            │              │                │              │
 what's      treemap +      profile         plan, size,     verified
 at risk     file table     checklist       passphrase      + restore
```

---

## How it works

```
┌───────────────┐   ┌──────────────┐   ┌─────────────┐   ┌──────────────┐
│  1. SCAN      │   │  2. CHOOSE   │   │  3. SEAL    │   │  4. VERIFY   │
│               │   │              │   │             │   │              │
│  raw $MFT     │──▶│  profiles +  │──▶│  Argon2id + │──▶│  re-hash     │
│  (or walkdir) │   │  hand-picked │   │  AES-256-GCM│   │  from disk   │
└───────────────┘   └──────────────┘   └─────────────┘   └──────────────┘
                                                                 │
                                                                 ▼
                                                        ┌──────────────┐
                                                        │  5. RESTORE  │
                                                        │  restore.ps1 │
                                                        │  (idempotent)│
                                                        └──────────────┘
```

### 1. Scanning — why it's fast

`walkdir` costs one directory-open plus one `stat` per file, and is dominated by kernel round-trips. A few million files takes minutes.

The **NTFS Master File Table** is a single, mostly-contiguous system file that already contains every name, parent pointer, size and timestamp on the volume. Reading it is one long sequential I/O.

So the scanner opens `\\.\C:` directly and parses the MFT itself — boot sector, `$MFT` data-run list, per-record update-sequence fixups, `$STANDARD_INFORMATION` / `$FILE_NAME` / `$DATA` attributes — streaming 8 MiB at a time.

```rust
// src-tauri/src/scanner/mft.rs
pub fn enumerate<F, P, C>(letter: char, sink: F, progress: P, cancel: C) -> AppResult<bool>
```

This needs **administrator rights**. Without them the app falls back to a parallel directory walk automatically and tells you it did. Same results, much slower on a big volume.

Results land in a struct-of-arrays index keyed by MFT record number, so parent lookups are a single array index and paths are reconstructed on demand rather than stored. A few million files costs a couple hundred MB of RAM instead of the ~1 GB a naive `Vec<PathBuf>` would need.

### 2. Choosing — profiles

A **profile** is a named bundle of glob patterns plus zero or more *secret actions* (things that need decryption rather than a file copy).

```jsonc
{
  "id": "opera-gx",
  "include": [
    "%APPDATA%/Opera Software/Opera GX Stable/**",
    "%LOCALAPPDATA%/Opera Software/Opera GX Stable/**"
  ],
  "exclude": ["**/Cache/**", "**/GPUCache/**"],
  "secrets": ["chromiumPasswords"]
}
```

`%ENV%` is expanded at plan time, not definition time, so a profile file is portable between machines. Built-ins are compiled in; your edits persist to `%APPDATA%\rebackup\profiles.json` and are merged over the built-ins by `id`, so upgrading keeps your changes.

### 3. Sealing — how the crypto works

```
 Opera / Chrome                        your passphrase
 "Local State"                                │
      │  base64 → strip "DPAPI" prefix        ▼
      ▼                               ┌───────────────┐
 CryptUnprotectData ──▶ 32-byte AES   │   Argon2id    │  m=64 MiB, t=3, p=1
      │                     key       │  16-byte salt │
      ▼                               └───────┬───────┘
 "Login Data" (SQLite)                        │ 32-byte key
   password_value = "v10" ‖ nonce ‖ GCM       ▼
      │                              ┌──────────────────┐
      ▼   AES-256-GCM decrypt        │   AES-256-GCM    │
  plaintext ──── in memory only ────▶│  KDF params      │──▶  *.rbu on disk
  (Zeroizing, wiped on drop)         │  bound as AAD    │
                                     └──────────────────┘
```

Concretely:

- **Nothing secret is ever written to disk in the clear.** Decrypted passwords live only in `Zeroizing` containers, are assembled into a CSV in memory, sealed, and dropped. There is no temp file to forget about.
- **KDF parameters are authenticated as AEAD associated data**, so an attacker can't edit `mCostKib` down to 8 KiB and have the blob still decrypt. There's a test for exactly that.
- **The manifest records `encrypted: true`, the algorithm and the KDF — never a plaintext, and never a hash of a secret** (hashing a short password just moves the problem).
- **A guard rail aborts the run** if any unsealed file ends up under `secrets/`.
- **`Login Data` is never opened in place.** It's copied along with its WAL, read read-only, then the copy is overwritten and unlinked — writing to the live DB would corrupt your browser profile.

### 4. Verifying

Every file is hashed *during* the copy (one read, not two), then re-hashed from disk afterwards and compared against the manifest. If anything mismatches, the report says **do not reset yet** in red.

This is not ceremony. It's the check that catches a dying USB stick *before* it costs you the data.

### 5. Restoring

The output folder is self-describing:

```
ReBackUp_20260811-174233/
├── files/C/Users/you/Desktop/...   mirrored source tree
├── secrets/*.rbu                   Argon2id + AES-256-GCM
├── manifest.json                   every path, size, SHA-256
├── report.html                     self-contained, opens offline
├── restore.ps1 / restore.cmd       idempotent restore driver
└── READ-ME-FIRST.txt               for future you
```

`restore.ps1` reads `manifest.json` at run time rather than having paths baked in, verifies each file's hash before writing it, and skips targets that already match. Re-running after a partial restore is safe and nearly free. It also rewrites `\Users\<oldname>\` to your new username — the one path component that reliably changes.

---

## Install

Download the `.msi` or NSIS `.exe` from [Releases](https://github.com/Aryansingh0783/rebackup/releases), or build from source below.

**Run it as administrator.** Reading the MFT requires it. The app works unelevated but falls back to the slow scanner and says so in the sidebar.

## Build from source

```bash
git clone https://github.com/Aryansingh0783/rebackup
cd rebackup
pnpm install
pnpm tauri dev        # development, hot reload
pnpm tauri build      # → src-tauri/target/release/bundle/
```

**Prerequisites**

| | |
|---|---|
| Node | 20+ |
| pnpm | 9+ |
| Rust | 1.82+ |
| Windows | Visual Studio Build Tools with the **C++ workload** *and* the **Windows SDK** |
| Runtime | WebView2 (already present on Windows 10 21H2+ and Windows 11) |

> If `cargo build` fails with `linker 'link.exe' not found`, you have the C++ tools but not the Windows SDK. Install it with:
> ```powershell
> winget install Microsoft.WindowsSDK.10.0.26100
> ```

## Usage

1. **Overview** — the app looks for browsers, Steam, git repos and stored credentials, and tells you which repos contain work that exists nowhere else. Read that list first; it's the part people forget.
2. **Scanner** — pick a drive, scan, click through the treemap. Filter by extension, size, date, substring or regex. Tick anything worth keeping that a profile doesn't already cover.
3. **What to keep** — things found on this machine are pre-ticked. Add custom globs for unusual layouts.
4. **Review & run** — choose a destination *on an external drive*, pick an archive format, set a passphrase, calculate the plan, run it.
5. **Result** — copy the folder off this machine, then verify **from the copy**.

### Before you reset — three checks, in this order

1. The backup folder is on an external drive, not on `C:`.
2. `rebackup.exe verify --manifest <path>` passes **from that external copy**.
3. Your passphrase is written down somewhere that isn't this computer.

## CLI

The GUI binary doubles as a small CLI, because `restore.ps1` runs on a machine that may not have a desktop session yet.

```
rebackup                       start the GUI
rebackup unseal --in <f.prb> --out <f> [--passphrase <p>]
rebackup verify --manifest <manifest.json>
rebackup shred  --path <file>
```

Omit `--passphrase` and it's read from stdin, keeping it out of the process list and your shell history.

---

## Security model

### What is protected

- Decrypted secrets exist only in `Zeroizing` memory and are wiped on drop.
- Sealing happens in memory; only the sealed blob touches the filesystem.
- Argon2id (64 MiB, t=3, p=1) + AES-256-GCM, KDF parameters authenticated so they can't be downgraded.
- Types that hold secrets have **no `Serialize` impl** — they physically cannot be sent to the UI or written to the manifest.
- A manifest audit refuses to finish a backup with an unsealed file under `secrets/`.
- Wrong passphrase and tampered ciphertext produce the *same* error, deliberately.

### What is not protected — read this

- **The staging folder is plaintext for everything that isn't a secret.** Your documents are documents. Encrypt the archive (7z + AES-256) or the drive if that matters.
- **Shredding is best-effort on an SSD.** Wear levelling may have already relocated the original blocks. The tool says so every time it runs.
- **Lose the passphrase, lose the sealed data.** No recovery, no backdoor, no key escrow.
- **DPAPI is scoped to your logon session.** This is why the tool works for *you on this machine* and cannot decrypt another account's profile or one from an offline disk. That's a property worth keeping, not a limitation to route around.
- **Chrome 127+ app-bound encryption is not defeated.** Those keys are sealed to a SYSTEM-level service. The app detects it, says so plainly, and routes you to the browser's own export instead of failing mysteriously.
- **`ssfn*` Steam sentry files are bound to machine + Windows SID.** After a clean install your SID changes. Back them up anyway — they cost nothing — but plan on re-authenticating.

### Attacker model

| Attacker | Outcome |
|---|---|
| Finds your backup drive | Sees your files; sealed artifacts need the passphrase, and Argon2id at 64 MiB makes bulk cracking expensive |
| Malware already running as you | Already has DPAPI access — this app changes nothing about that |
| Another user on the same PC | Cannot decrypt your DPAPI blobs, cannot read another account's profile |
| Someone with your old disk, offline | Cannot use DPAPI at all; sealed blobs still need the passphrase |

Found a hole? See [SECURITY.md](SECURITY.md). Please don't attach real manifests or `.prb` files to an issue — manifests contain full filesystem paths, and paths leak more than people expect.

---

## Project layout

```
rebackup/
├── src-tauri/src/
│   ├── scanner/
│   │   ├── mft.rs        raw NTFS MFT reader — boot sector, run-lists,
│   │   │                 fixups, $FILE_NAME/$DATA. The fast path.
│   │   ├── walk.rs       parallel directory walk (portable fallback)
│   │   ├── index.rs      struct-of-arrays index + CSR children + treemap
│   │   └── mod.rs        backend selection, pause/resume/cancel, events
│   ├── secrets/
│   │   ├── dpapi.rs      CryptUnprotectData / CryptProtectData
│   │   ├── chromium.rs   Local State key + Login Data decryption
│   │   ├── vault.rs      Credential Manager inventory + guided export
│   │   └── mod.rs        sealing, shredding, invariants
│   ├── crypto.rs         Argon2id + AES-256-GCM envelope
│   ├── profiles.rs       declarative "what to take"
│   ├── backup.rs         plan → stage → seal → archive → verify
│   ├── manifest.rs       manifest + verification
│   ├── restore.rs        restore.ps1 generation
│   ├── report.rs         self-contained HTML report
│   ├── steam.rs          Steam discovery      vdf.rs   Valve KeyValues parser
│   ├── git.rs            repo + credential-helper discovery
│   ├── opera.rs          browser profile detection
│   ├── cli.rs            unseal / verify / shred
│   └── lib.rs            every Tauri command
├── src/                  React 19 + TypeScript + Tailwind
│   ├── components/       TreemapView (canvas), FileTable (virtualised), …
│   ├── stores/           zustand
│   └── lib/api.ts        typed command wrappers
└── docs/                 VitePress site
```

**~7,000 lines of Rust, ~2,500 of TypeScript, 58 unit tests.**

---

## Engineering decisions

Places where the obvious choice was rejected, and why. Each was deliberate.

| Instead of | This does | Why |
|---|---|---|
| The `dpapi-core` crate | Direct `CryptUnprotectData` via `windows` | `dpapi-core` implements **DPAPI-NG** (MS-GKDI group keys for LAPS/AD), a different protocol from the per-user DPAPI that Chromium uses. The Win32 call is ~40 lines with no extra supply-chain surface. |
| An off-the-shelf MFT crate | A hand-written parser | Keeps the parse loop, fixup handling and run-list decoder auditable and unit-tested, with no dependency on a thinly-maintained crate for the single most correctness-critical path in the app. |
| Driving the browser's Export button over `--remote-debugging-port` | Direct DPAPI decryption, with a guided manual export as fallback | Opening a debug port lets **any** local process drive the browser and read its cookies for as long as it's open. Trading a real security hole for UI convenience is a bad deal when direct decryption gets the same data. |
| `vaultcmd /backup` | `rundll32 keymgr.dll,KRShowKeyMgr` + step-by-step guidance | `vaultcmd` has no `/backup` verb on modern Windows. The wizard is the supported path, and it *requires* the secure desktop precisely so automation can't drive it. Credentials are inventoried automatically; the export is guided. |
| `tauri-plugin-fs` / `tauri-plugin-shell` | Neither plugin is loaded | Those plugins grant the **webview** filesystem and process access. Every file operation already runs inside a purpose-built Rust command, so enabling them would widen the attack surface and buy nothing. The capability file grants only `dialog` and `opener`. |
| A charting library's treemap | Canvas squarified treemap | 2,000+ SVG nodes with hover handlers janks the window. Canvas plus a hit-test array holds 60 fps, and squarification (Bruls/Huizing/van Wijk) keeps rectangles comparable — the thing naive slice-and-dice gets wrong. |
| ZIP as the light archive | zip + zstd, **explicitly labelled unencrypted** | Legacy ZipCrypto is broken. Better to not claim encryption than to imply it. Sealed artifacts inside stay sealed regardless. |
| Rust 2024 edition | Rust 2021, MSRV 1.82 | 2024 needs rustc ≥ 1.85 and this crate uses nothing from it. Lower MSRV, more contributors. |

---

## Project status

**v0.0.0 — early. Works, but under-tested in the wild.**

| | |
|---|---|
| `cargo check --all-targets --all-features` | clean |
| `cargo clippy -- -D warnings` | clean |
| `cargo test --all-features` | 58 / 58 |
| `tsc --noEmit` | clean |
| Runs and detects real browsers / Steam / repos / credentials | ✅ verified on Windows 11 |
| MFT fast-path timing on a 1 TB NVMe | ⚠️ **not yet measured** |
| Full restore on a clean Windows 11 VM | ⚠️ **not yet performed** |
| Encrypted 7z round-trip on a large dataset | ⚠️ **not yet performed** |

The unverified rows are marked unverified on purpose. Claiming a benchmark nobody ran is how a backup tool loses someone's data and their trust in the same afternoon.

**Most useful contributions right now:** a verified restore run on a clean VM with notes, real MFT timings across drive sizes, and additional profiles (password managers, VPN configs, licence files).

## Non-goals

- Cloud sync. Local-only, deliberately.
- Full-disk imaging — use Macrium Reflect or Clonezilla.
- Decrypting another account's DPAPI blobs, or an offline disk.
- Browsers with no export or decryption path.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). There are hard rules for anything touching secret material — decrypted values live in `Zeroizing`, nothing secret goes to disk, a log line, an error, or the manifest, and every such change needs a test proving the secret didn't leak.

## License

MIT — see [LICENSE](LICENSE).
