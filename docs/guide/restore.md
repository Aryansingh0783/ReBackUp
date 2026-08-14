# Restoring

## Files

```powershell
.\restore.ps1 -DryRun          # preview
.\restore.cmd                  # do it
.\restore.ps1 -Only desktop    # one profile
.\restore.ps1 -Force           # overwrite files that differ
```

The script is idempotent: it verifies each staged file's SHA-256 before writing, and skips targets that already match. Re-running after a partial failure is safe and cheap.

It also rewrites `\Users\<oldname>\` to your new username automatically, which is the one path component that reliably changes.

## Browser passwords

```powershell
rebackup.exe unseal --in secrets\opera-gx-default-passwords.csv.rbu --out passwords.csv
```

Then in the browser: password manager settings → **Import** → pick the CSV. Finally:

```powershell
rebackup.exe shred --path passwords.csv
```

Restoring the profile folder brings back tabs, bookmarks, extensions and sessions. Only passwords need the CSV round-trip — the profile's own copies are encrypted to the *old* Windows account and are unrecoverable.

## Steam

1. Install Steam, then **close it completely** (check the tray).
2. Restore `config\loginusers.vdf`, `config\config.vdf` and the `ssfn*` files.
3. Start Steam. Expect a Steam Guard prompt — sentry files are bound to the old machine *and* the old user SID.
4. Point Steam at your existing library folders (Settings → Storage) instead of re-downloading everything.

## Git

```powershell
git config --global credential.helper manager
icacls "$env:USERPROFILE\.ssh\id_ed25519" /inheritance:r /grant:r "$env:USERNAME:R"
```

OpenSSH refuses keys with permissive ACLs, and a restored key inherits the wrong ones — that `icacls` line is the fix for the error you'll otherwise hit on first push.

If you restored `~/.git-credentials`, **rotate those tokens.** They were stored in plaintext.

## Windows Credentials

```
rundll32.exe keymgr.dll,KRShowKeyMgr
```

Choose **Restore Credentials**, pick your `.crd`, press Ctrl+Alt+Delete when prompted, and enter the protection password you set during backup.
