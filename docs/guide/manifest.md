# Manifest format

`manifest.json` is written last; its presence means the run completed.

```jsonc
{
  "version": 1,
  "tool": "rebackup",
  "toolVersion": "0.1.0",
  "created": "2026-08-11T17:42:33Z",
  "machine": "DESKTOP-ABC",
  "user": "you",
  "windowsBuild": "Windows 11 Pro 24H2 (build 26100)",
  "stagingRoot": "E:\\ReBackUp_20260811-174233",
  "profiles": ["desktop", "opera-gx", "steam"],

  "entries": [
    {
      "source": "C:\\Users\\you\\Desktop\\notes.md",
      "staged": "files/C/Users/you/Desktop/notes.md",
      "bytes": 4096,
      "sha256": "9f86d081…",
      "modified": 1754931753,
      "profile": "desktop"
    }
  ],

  "sealed": [
    {
      "path": "E:\\…\\secrets\\opera-gx-default-passwords.csv.rbu",
      "label": "Opera GX Default passwords (Chromium CSV)",
      "items": 142,
      "sha256": "3b1f…",
      "encrypted": true,
      "algorithm": "aes-256-gcm",
      "kdf": "argon2id"
    }
  ],

  "archive": { "path": "…7z", "format": "7z/LZMA2", "encrypted": true },
  "skipped": [{ "path": "…", "reason": "cannot stat: access denied" }],
  "warnings": ["Steam: sentry files are bound to this machine AND user SID…"],
  "context": { "steam": {}, "git": {}, "browsers": [], "credentials": [] }
}
```

## Invariants

- `entries[].sha256` is computed **during** the copy and re-checked afterwards.
- `sealed[]` never contains plaintext — only metadata about the encrypted blob.
- No entry under `secrets/` may lack a `.prb` extension. The audit enforces this and aborts the run.
- `context` is informational: Steam accounts, git repos, browser profiles, credential inventory. It is what the HTML report renders.

## Sealed blob (`.prb`)

```jsonc
{
  "magic": "RBU1",
  "cipher": "aes-256-gcm",
  "kdf": { "algorithm": "argon2id", "mCostKib": 65536, "tCost": 3, "pCost": 1 },
  "salt": "<base64, 16 bytes>",
  "nonce": "<base64, 12 bytes>",
  "ciphertext": "<base64, includes the 16-byte GCM tag>",
  "label": "Opera GX Default passwords (Chromium CSV)"
}
```

`magic`, the KDF parameters and `label` are fed to AES-GCM as associated data. Editing any of them makes decryption fail — you cannot downgrade `mCostKib` to make cracking cheap.
