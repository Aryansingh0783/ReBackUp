# Contributing

Thanks for looking. This tool handles passwords, session tokens and private keys on the way to a machine that's about to be wiped — a bug here loses data that has no second copy. The bar is higher than usual, and the rules below exist because of that.

## Getting set up

```bash
pnpm install
pnpm tauri dev
cd src-tauri && cargo test --all-features
```

CI gates on `cargo fmt --all -- --check`, so run the formatter once before your
first commit — it's the cheapest red build to avoid:

```bash
cd src-tauri && cargo fmt --all
```

Run everything CI runs before you push:

```bash
pnpm typecheck && pnpm lint
cd src-tauri && cargo fmt --all -- --check
cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings
cd src-tauri && cargo test --all-features && cargo test --no-default-features
```

## Rules for code that touches secrets

Non-negotiable. A PR that breaks one of these gets sent back regardless of how good the rest is.

1. **Decrypted material lives in `Zeroizing` (or a type that wraps it).** No bare `String`, no bare `Vec<u8>`.
2. **Never write a plaintext secret to disk.** Seal in memory. If you think you need a temp file, you need a different design.
3. **Never put a secret in an error, a log line, an event payload, or the manifest.** Counts, lengths and identifiers only. Not even a hash — short secrets are crackable.
4. **No `Serialize` on a type that holds a secret.** `LoginRecord` deliberately doesn't have one. Keep it that way.
5. **Add a test that proves the secret didn't leak.** `crypto::tests::plaintext_never_appears_in_the_serialised_blob` is the pattern: seal a known sentinel, assert the artifact doesn't contain it.
6. **No hand-rolled cryptography.** Use the primitives in `crypto.rs`. If you need a new one, open an issue first.

## Rules for the scanner

The MFT parser reads raw disk structures. Malformed input is expected, not exceptional.

- Bounds-check every offset before you index. `parse_record` is written the way it is on purpose.
- Never panic on bad on-disk data — skip the record and keep going. A corrupt file must not abort a 3M-file scan.
- New parsing logic needs a unit test with hand-built bytes. See `decode_runs` and `apply_fixups` for the shape.

## Pull requests

- One concern per PR.
- Explain **why**, not what. The diff shows what.
- Update the README's *Deviations* table if you deliberately diverge from something documented.
- New profiles need: a detector in `detect.rs`, an entry in `profiles::builtin()`, a follow-up section in `assets/restore.ps1.tmpl`, and a note about anything that *won't* survive the reset.
- Comments explain reasoning, not mechanics. `// increment i` helps nobody; `// $FILE_NAME's size is stale for files with an $ATTRIBUTE_LIST` helps a lot.

## Testing on real data

Test with a throwaway Windows VM and a throwaway browser profile with fake passwords. Don't use your real profile to develop against — a mistake in the shred or copy path is expensive, and you'll be tempted to skip the passphrase.

## What's especially welcome

- A verified restore run on a clean Windows 11 VM, with notes.
- Real MFT timings across drive sizes and filesystems.
- Additional profiles (password managers, VPN configs, licence files, Office macros).
- Making the walkdir fallback faster, so unelevated users get a better experience.
- Anything that removes a "trust me" from the README and replaces it with a test.
