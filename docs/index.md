---
layout: home
hero:
  name: ReBackUp
  text: Know what you're about to lose
  tagline: A WizTree-style scanner and a profile-driven, verified, encrypted backup — for the hour before you wipe Windows.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: Security model
      link: /guide/security
features:
  - title: Reads the MFT directly
    details: One sequential pass over the NTFS Master File Table instead of millions of stat calls. Falls back to a parallel directory walk when it can't.
  - title: Knows where things hide
    details: Opera GX passwords, Steam sentry files, git repos with unpushed work, Cursor and VS Code settings, Windows credentials — found automatically.
  - title: Encrypts what deserves it
    details: Argon2id + AES-256-GCM. Decrypted secrets never touch the disk, and the manifest refuses to publish if one does.
  - title: Verifies before you commit
    details: SHA-256 per file at copy time and again afterwards. If it doesn't verify, the app says so loudly rather than quietly.
---
