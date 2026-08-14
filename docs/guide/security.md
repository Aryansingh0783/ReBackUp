# Security model

The full model lives in the repository's [README](https://github.com/Aryansingh0783/rebackup#security-model) and [SECURITY.md](https://github.com/Aryansingh0783/rebackup/blob/main/SECURITY.md). The short version:

## Protected

- Decrypted secrets exist only in `Zeroizing` memory and are wiped on drop.
- Sealing happens in memory. Only the sealed blob touches disk.
- Argon2id (64 MiB, t=3, p=1) + AES-256-GCM, with KDF parameters authenticated as associated data so they can't be downgraded.
- The manifest records `encrypted: true` and the algorithm — never a plaintext, never a hash of a secret.
- A guard rail aborts the run if any unsealed file lands under `secrets/`.
- `Login Data` is copied (with its WAL) and read read-only; the copy is overwritten and unlinked.

## Not protected

- The staging folder is plaintext for ordinary files. Encrypt the archive or the drive.
- Shredding is best-effort on SSDs — wear levelling may have relocated the blocks already.
- Lose the passphrase, lose the sealed data. No recovery.
- DPAPI is scoped to your logon session. Other accounts and offline disks are out of reach — deliberately.
- Chrome 127+ app-bound encryption is not defeated. The app detects it and routes you to the browser's own export.

## Reporting

GitHub Security Advisories → *Report a vulnerability*. Never attach real manifests, `.prb` files or browser data — synthesise a repro.
