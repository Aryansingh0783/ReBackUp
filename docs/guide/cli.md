# CLI

The GUI binary doubles as a small CLI, because `restore.ps1` runs on a machine that may not have a desktop session yet.

```
pre-reset-backup                       start the GUI
pre-reset-backup unseal --in <f.prb> --out <f> [--passphrase <p>]
pre-reset-backup verify --manifest <manifest.json>
pre-reset-backup shred  --path <file>
pre-reset-backup --version
```

## unseal

```powershell
pre-reset-backup.exe unseal --in secrets\opera-gx-default-passwords.csv.prb --out passwords.csv
```

Omit `--passphrase` and it's read from stdin — that keeps it out of the process list and your shell history. Refuses to overwrite an existing output file.

A wrong passphrase and a tampered blob give the same error, on purpose.

## verify

```powershell
pre-reset-backup.exe verify --manifest E:\PreResetBackup_20260811-174233\manifest.json
```

Re-hashes every staged file and sealed artifact. Exit code 0 means intact, 1 means don't trust it. Prints `MISMATCH <path>` and `MISSING <path>` on stdout so you can pipe it.

**Run this against the copy on your external drive**, not the original — it verifies the copy at the same time.

## shred

```powershell
pre-reset-backup.exe shred --path passwords.csv
```

Two overwrite passes, then unlink. On an SSD, wear levelling may have relocated the original blocks — the command says so every time it runs. Treat it as best-effort hygiene, not a guarantee.
