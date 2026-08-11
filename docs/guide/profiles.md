# Profiles

A profile is a named bundle of glob patterns plus zero or more *secret actions* — things needing decryption rather than a file copy.

## Patterns

```
%APPDATA%/Opera Software/Opera GX Stable/**
%USERPROFILE%/Projects/**/*.psd
%PROGRAMFILES(X86)%/Steam/ssfn*
```

- `**` matches any number of path segments; `*` stays within one.
- `%ENV%` and a leading `~` are expanded at plan time, never at definition time — so a profile file is portable between machines.
- Matching is case-insensitive, because Windows is.

## Global excludes

Caches are dropped from every profile: `Cache`, `Code Cache`, `GPUCache`, `ShaderCache`, `Crashpad`, `node_modules`, `__pycache__`, `Thumbs.db`, `*.tmp`. They're large, they're regenerated on first launch, and copying them just makes the backup slower.

## Secret actions

| Action | What it does |
|---|---|
| `chromiumPasswords` | DPAPI → AES-GCM decrypt of `Login Data`, re-sealed as a CSV |
| `windowsVault` | Credential Manager inventory + guided `.crd` export |
| `gitCredentials` | Seals `~/.git-credentials` and private SSH keys |
| `steamSentry` | Seals `ssfn*` files |

## Custom profiles

Add patterns in the wizard, or edit `%APPDATA%\pre-reset-backup\profiles.json` directly. Your edits are merged over the built-ins by `id`, so upgrading the app keeps your changes and still picks up new built-in profiles.

## Adding a built-in

Four places, all of them required:

1. `detect.rs` — a detector so the wizard can pre-tick it.
2. `profiles::builtin()` — the patterns.
3. `assets/restore.ps1.tmpl` — the manual follow-up steps.
4. `notes` — anything that **won't** survive the reset. This is the part users actually need.
