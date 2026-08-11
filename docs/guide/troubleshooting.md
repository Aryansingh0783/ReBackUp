# Troubleshooting

## "Fell back to the slow scanner: access denied"

You aren't elevated. Close the app and run it as administrator. The MFT path needs raw volume access (`\\.\C:`), which requires Administrators membership or `SeBackupPrivilege`.

## "volume X: is not NTFS"

The MFT reader only works on NTFS. exFAT, FAT32 and ReFS use the walk backend automatically — nothing is wrong.

## "no decryptable logins … blocked by app-bound encryption"

The browser uses Chrome 127+ app-bound encryption. Those keys are sealed to a SYSTEM-level service and are not readable from a user process by design.

Export manually: open the browser's password manager settings (the app shows the exact URL), choose **Export**, then use *Seal exported CSV* — the app encrypts it with your passphrase and shreds the plaintext.

## "CryptUnprotectData failed"

DPAPI blobs are bound to a Windows logon session. This happens when the profile was copied from another machine or belongs to a different account. There's no way around it, and that's the point of DPAPI.

## "Login Data schema mismatch"

The browser version stores passwords in a table layout we don't recognise. Open an issue with the browser name and version — **not** the file.

## Scan finds far fewer files than expected

Check whether the walk backend ran (the summary line says which). The walk fallback honours `same_file_system`, so it won't cross into mounted volumes or junctions pointing elsewhere. Scan those separately.

## Backup verification failed

Do not reset. Re-run the backup. If it fails again in the same place, suspect the destination drive — run `chkdsk` on it, or pick a different one. This check exists precisely to catch a dying USB stick before it costs you the data.

## Restore says "exists and differs, skipping"

A file is already present with different contents. Pass `-Force` to overwrite, or move the existing file aside first. The default is conservative on purpose.

## SSH key rejected after restore

```powershell
icacls "$env:USERPROFILE\.ssh\id_ed25519" /inheritance:r /grant:r "$env:USERNAME:R"
```

OpenSSH refuses private keys with permissive ACLs, and restored files inherit the target directory's.
