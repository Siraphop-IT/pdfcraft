# BMU PDF on Windows: installing and updating

BMU PDF installs **for the signed-in account only**, into `%LOCALAPPDATA%\Programs\BMU PDF`, and
needs no administrator. That is what lets it update itself: the person using it answers one
question and nobody types an administrator password.

## Updating (every release after the first)

1. Build: push `bmu/rebrand`; the **BMU PDF for Windows** workflow numbers the release
   (`0.18.<run number>`) and uploads `bmu-pdf-<version>-windows-x64.msi`.
2. Download it, then publish it to the NAS:

   ```powershell
   ./packaging/windows/bmu-deploy-update.ps1 -Msi 'D:\BMU PDF Setup\bmu-pdf-0.18.42-windows-x64.msi' -Notes 'What changed'
   ```

   The script signs `update.json` with the release key (`cargo xtask bmu-update sign`), uploads
   the installer, `update.json` and `update.json.sig` to `/web/BMU_Drive/bmu-pdf` (served as
   `http://<NAS>:8888/bmu-pdf/`), and reads `update.json` back. It logs in with BMU Drive's
   settings on the release machine.
3. Every copy asks at its next start (or on Help ▸ Check for updates): *A new version of BMU PDF
   is available*. **Update now** needs every document saved and BMU PDF's other windows closed;
   BMU PDF then closes, Windows Installer replaces it with a progress bar, and it opens again.

The app tries the NAS on the office network first (`192.168.1.100:8888`), then its internet name.
It believes only an `update.json` whose signature matches the release key built into it, and
runs only an installer whose size and SHA-256 match that signed file
(`apps/pdfcraft/src/update.rs`).

### The release key

`cargo xtask bmu-update keygen --key FILE` made it once; it lives at
`%USERPROFILE%\BMU PDF Release\update-signing.key` on the release machine, readable by that
account only. **Keep a backup somewhere safe and private.** Without it no new release can be
published to the installed copies (they would need reinstalling by hand with a new key); anyone
who has it can publish one. Never commit it or put it on the NAS.

## Moving a PC from the old all-users install (once per PC)

Releases up to 0.18.0 installed for all users into `C:\Program Files\BMU PDF`. Windows Installer
cannot turn that into a per-user copy, and the new installer refuses to install beside it
("BMU PDF is installed for all users on this computer…"). On each PC, once:

1. An administrator uninstalls **BMU PDF** in Settings ▸ Apps (or
   `msiexec /x "bmu-pdf-0.18.0-windows-x64.msi"`). Documents, settings and signatures stay: they
   are kept in the user's profile, not in Program Files.
2. The user, signed in as themselves, runs the new `bmu-pdf-<version>-windows-x64.msi`. No
   administrator password is asked for.
3. If BMU PDF should open PDFs by default, the user chooses it again in Settings ▸ Default apps
   (the choice is per account now).

From then on, updates arrive as above.
