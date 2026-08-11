# Security Policy

## Reporting a vulnerability

Use **GitHub Security Advisories** → *Report a vulnerability* on this repository. Please don't open a public issue for anything exploitable.

Include: what you found, how to reproduce it, and what an attacker gets. A proof of concept helps enormously. Expect an acknowledgement within 72 hours and a fix or a plan within 14 days for anything that leaks secret material.

**Never attach real data.** No `manifest.json`, no `.prb` files, no `Local State`, no `Login Data`, no `.crd`. Manifests contain full filesystem paths and paths leak more than people expect. Synthesise a repro.

## Scope

In scope — anything that:

- writes secret material to disk unencrypted, however briefly;
- puts a plaintext secret into a log line, an error message, an event payload, or the manifest;
- lets a blob be decrypted without the passphrase, or lets KDF parameters be downgraded;
- lets a crafted filename, path, or profile escape the staging directory (path traversal);
- lets a malicious backup folder execute code during restore;
- weakens the Tauri capability set beyond what a command actually needs.

Out of scope:

- The staging folder being readable by the user who created it. That's the design.
- Not defeating Chrome 127+ app-bound encryption. That's the design.
- Shredding not being guaranteed on SSDs. Documented, and the tool says so at runtime.
- Requiring elevation for the MFT fast path. That's Windows.

## Guarantees this project makes

1. Decrypted secrets exist only in `Zeroizing` containers and are wiped on drop.
2. Sealing happens in memory; only the sealed blob touches the filesystem.
3. `AppError` variants carry no secret material — errors are shown in the UI and written to logs.
4. Types that hold secrets (`LoginRecord`, `SecretString`) have no `Serialize` impl.
5. The manifest audit refuses to finish a backup with an unsealed file under `secrets/`.
6. KDF parameters are authenticated as AEAD associated data.
7. Wrong passphrase and tampered ciphertext are indistinguishable in the error.

Each of these has a unit test. If you break one, CI should catch it — if it doesn't, that gap is itself a bug worth reporting.

## Cryptography

| Purpose | Primitive | Parameters |
|---|---|---|
| Key derivation | Argon2id | m = 64 MiB, t = 3, p = 1, 16-byte salt |
| Sealed artifacts | AES-256-GCM | 96-bit random nonce, 128-bit tag, KDF params as AAD |
| Integrity | SHA-256 | per file, at copy time and again at verify |
| Archive (7z) | AES-256 | 7-Zip's own KDF (SHA-256, 2^19 iterations) |
| Chromium values | AES-256-GCM | key from `Local State` via DPAPI |

No custom cryptography. If you find some, that's a bug.

## Supported versions

Only the latest release. This is a tool you run a handful of times, not a service.
