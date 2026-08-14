# Getting started

## Install

Grab the `.msi` or the NSIS `.exe` from [Releases](https://github.com/Aryansingh0783/rebackup/releases), or build it yourself:

```bash
pnpm install
pnpm tauri build
```

## Run it elevated

Right-click → **Run as administrator**. Reading the NTFS Master File Table needs it.

Without elevation everything still works — the scanner falls back to a directory walk. On a 1 TB drive that's the difference between seconds and minutes. The sidebar tells you which mode you're in.

## The five-minute path

1. **Overview** — the app looks for browsers, Steam, git repos and stored credentials, and tells you which repos have work that exists nowhere else. Read that list first; it's the part people forget.
2. **Scanner** — pick a drive, hit scan, click through the treemap. Tick anything worth keeping that isn't covered by a profile.
3. **What to keep** — things found on this machine are already ticked. Add custom globs if you have an unusual layout.
4. **Review & run** — pick a destination *on an external drive*, choose an archive format, set a passphrase, calculate the plan, run it.
5. **Result** — copy the folder off this machine, then verify from the copy.

## Before you reset

Three things, in this order:

1. The backup folder is on an external drive, not on `C:`.
2. `rebackup.exe verify --manifest <path>` passes **from that external copy**.
3. Your passphrase is written down somewhere that isn't this computer.

Then, and only then, reset Windows.
