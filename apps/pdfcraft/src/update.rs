//! BMU PDF updates itself from the office NAS (Windows). The NAS's web server holds the newest
//! installer, an `update.json` naming it, and `update.json.sig`, an Ed25519 signature of that file
//! made with the office's release key (`cargo xtask bmu-update sign`). The app reads both, checks
//! the signature against the public key built in here, and offers the version when it is newer
//! than its own. Installing downloads the MSI the manifest names, checks its size and SHA-256
//! against the signed manifest, and starts Windows Installer on it, which replaces this per-user
//! copy without asking for an administrator.
//!
//! Nothing here trusts the network: the NAS answers over plain HTTP, so only what the signature
//! covers is believed. A manifest for an older or equal version, another product, or with a
//! signature that doesn't verify is never offered, and an installer that differs from the signed
//! manifest in a single byte is deleted unrun.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

/// The office's release key (public half). Its private half signs `update.json` and never
/// leaves the release machine (`cargo xtask bmu-update public --key FILE` prints this line).
pub const PUBLIC_KEY: [u8; 32] = [
    0xb4, 0x87, 0x34, 0xa2, 0xdd, 0x4e, 0x86, 0xa5, 0x3f, 0xcf, 0x64, 0xac, 0x89, 0x1d, 0xe9, 0xf0, 0xde, 0x52, 0xa8, 0x52, 0x4c, 0x10, 0x17, 0x60,
    0xc3, 0x24, 0xe4, 0x82, 0xdd, 0x09, 0x74, 0x22,
];

/// Where the updates are, tried in order: the NAS on the office network first, then the same NAS
/// through its internet name (for laptops away from the office).
pub const SOURCES: &[&str] = &["http://192.168.1.100:8888/bmu-pdf/", "http://buildmeupconsultant.synology.me:8888/bmu-pdf/"];

/// What a manifest says it is for; a manifest signed for anything else is refused.
const PRODUCT: &str = "bmu-pdf-windows-x64";

/// Longest manifest or signature read.
const MANIFEST_LIMIT: u64 = 64 * 1024;
/// Largest installer a manifest may name.
const INSTALLER_LIMIT: u64 = 1 << 30;
/// Longest release note kept, in characters.
const NOTES_LIMIT: usize = 4000;

/// A release version: three numbers, as Windows Installer's product versions are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u32, u32, u32);

impl Version {
    /// `0.18.42`; anything else (a fourth part, a suffix, blanks) is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut next = || -> Option<u32> {
            let part = parts.next()?;
            (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())).then(|| part.parse().ok())?
        };
        let version = Version(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A release a verified manifest describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The installer's file name, beside the manifest.
    pub file: String,
    pub size: u64,
    pub sha256: [u8; 32],
    /// What changed, for the person deciding whether to update.
    pub notes: String,
    /// The address the manifest came from, which the installer is downloaded from too.
    pub source: String,
}

/// Check `manifest` against `signature` (hex) with `key`, and read the release it describes.
pub fn verify(manifest: &[u8], signature: &str, key: &VerifyingKey) -> Result<Release, String> {
    let signature = decode_hex::<64>(signature.trim()).ok_or("the update signature is not 64 bytes of hex")?;
    key.verify_strict(manifest, &Signature::from_bytes(&signature))
        .map_err(|_| "the update manifest's signature does not match BMU PDF's release key")?;
    let json: serde_json::Value = serde_json::from_slice(manifest).map_err(|e| format!("the update manifest is not JSON ({e})"))?;
    let text = |name: &str| json.get(name).and_then(serde_json::Value::as_str);
    if text("product") != Some(PRODUCT) {
        return Err(format!("the update manifest is not for {PRODUCT}"));
    }
    let version = text("version").and_then(Version::parse).ok_or("the update manifest has no version")?;
    let file = text("file").filter(|f| plain_installer_name(f)).ok_or("the update manifest names no installer")?;
    let size = json
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .filter(|s| (1..=INSTALLER_LIMIT).contains(s))
        .ok_or("the update manifest has no installer size")?;
    let sha256 = text("sha256").and_then(decode_hex::<32>).ok_or("the update manifest has no installer SHA-256")?;
    let notes = text("notes").unwrap_or_default().chars().take(NOTES_LIMIT).collect();
    Ok(Release { version, file: file.to_owned(), size, sha256, notes, source: String::new() })
}

/// A file name the installer can be saved under and fetched by: letters, digits, `.`, `-` and
/// `_`, ending in `.msi`, so it can't reach outside the update folder or the NAS folder.
fn plain_installer_name(name: &str) -> bool {
    (5..=128).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && !name.starts_with('.')
        && name.to_ascii_lowercase().ends_with(".msi")
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut out = [0; N];
    for (byte, &[high, low]) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        let digit = |b: u8| (b as char).to_digit(16);
        *byte = u8::try_from(digit(high)? * 16 + digit(low)?).ok()?;
    }
    Some(out)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(4)).timeout_read(Duration::from_secs(30)).redirects(0).build()
}

/// GET `url`, refusing a body longer than `limit`.
fn fetch(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut body = Vec::new();
    response.into_reader().take(limit.saturating_add(1)).read_to_end(&mut body).map_err(|e| format!("{url}: {e}"))?;
    if u64::try_from(body.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("{url}: longer than {limit} bytes"));
    }
    Ok(body)
}

/// The newest release at `sources` (the first one that answers with a verified manifest), when
/// it is newer than `current`.
pub fn check_at(sources: &[&str], key: &VerifyingKey, current: Version) -> Result<Option<Release>, String> {
    let agent = agent();
    let mut failure = String::from("no update source");
    for source in sources {
        let manifest = fetch(&agent, &format!("{source}update.json"), MANIFEST_LIMIT);
        let signature = manifest.and_then(|m| Ok((m, fetch(&agent, &format!("{source}update.json.sig"), MANIFEST_LIMIT)?)));
        let release = signature.and_then(|(m, s)| verify(&m, &String::from_utf8_lossy(&s), key));
        match release {
            Ok(release) => return Ok((release.version > current).then(|| Release { source: (*source).to_owned(), ..release })),
            Err(e) => {
                log::info!("update check: {e}");
                failure = e;
            }
        }
    }
    Err(failure)
}

/// Download `release`'s installer into `dir`, checking it against the manifest as it arrives.
/// `progress` hears the bytes so far and the total. A file that doesn't match is removed.
pub fn download(release: &Release, dir: &Path, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(&release.file);
    let part = dir.join(format!("{}.part", release.file));
    let result = download_to(release, &part, progress).and_then(|()| std::fs::rename(&part, &path).map_err(|e| format!("{}: {e}", path.display())));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result.map(|()| path)
}

fn download_to(release: &Release, part: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let url = format!("{}{}", release.source, release.file);
    let response = agent().get(&url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut body = response.into_reader().take(release.size.saturating_add(1));
    let mut file = std::fs::File::create(part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let n = body.read(&mut buffer).map_err(|e| format!("{url}: {e}"))?;
        let Some(chunk) = buffer.get(..n).filter(|c| !c.is_empty()) else { break };
        done = done.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        if done > release.size {
            return Err(format!("{url}: longer than the update manifest says"));
        }
        hash.update(chunk);
        file.write_all(chunk).map_err(|e| format!("{}: {e}", part.display()))?;
        progress(done, release.size);
    }
    file.sync_all().map_err(|e| format!("{}: {e}", part.display()))?;
    if done != release.size {
        return Err(format!("{url}: {done} bytes, the update manifest says {}", release.size));
    }
    if hash.finalize().as_slice() != release.sha256 {
        return Err(format!("{url}: the installer differs from the update manifest"));
    }
    Ok(())
}

/// Start Windows Installer on `msi` (passive: a progress bar and no questions), then BMU PDF again
/// at `relaunch` once it is done. The caller closes the app so its files can be replaced.
pub fn install(msi: &Path, relaunch: Option<&Path>) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    let windows = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let msiexec = windows.join("System32").join("msiexec.exe");
    let install = format!("\"{}\" /i \"{}\" /passive /norestart", msiexec.display(), msi.display());
    // cmd runs the two in turn; it expands %…% even inside quotes, so a path holding one is
    // installed without the restart rather than mangled.
    let line = match relaunch.map(|p| p.display().to_string()).filter(|p| !p.contains('%') && !install.contains('%')) {
        Some(exe) => format!("/d /s /c \"{install} & start \"\" \"{exe}\"\""),
        None => format!("/d /s /c \"{install}\""),
    };
    std::process::Command::new(windows.join("System32").join("cmd.exe"))
        .raw_arg(line)
        // CREATE_NO_WINDOW: no console flashes up behind Windows Installer's progress bar.
        .creation_flags(0x0800_0000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

/// The app's [`Updater`](pdfcraft_ui_egui::Updater): only release builds, which know their release
/// number, have one. `at_start` checks once when the window opens.
pub fn updater(at_start: bool) -> Option<pdfcraft_ui_egui::Updater> {
    let current = Version::parse(pdfcraft_engine::build_info::RELEASE?)?;
    let key = VerifyingKey::from_bytes(&PUBLIC_KEY).ok()?;
    // The release the last check found, which Update now installs.
    let found: Arc<Mutex<Option<Release>>> = Arc::default();
    let keep = Arc::clone(&found);
    Some(pdfcraft_ui_egui::Updater {
        version: current.to_string(),
        check: Arc::new(move || {
            let release = check_at(SOURCES, &key, current)?;
            let offer = release.as_ref().map(|r| pdfcraft_ui_egui::UpdateOffer { version: r.version.to_string(), notes: r.notes.clone() });
            *keep.lock().unwrap_or_else(PoisonError::into_inner) = release;
            Ok(offer)
        }),
        install: Arc::new(move |offer, progress| {
            let release = found.lock().unwrap_or_else(PoisonError::into_inner).clone();
            let release = release.filter(|r| r.version.to_string() == offer.version).ok_or("Check for updates again.")?;
            let dir = std::env::temp_dir().join("BMU PDF update");
            // Installers from earlier updates are done with.
            let _ = std::fs::remove_dir_all(&dir);
            let msi = download(&release, &dir, progress)?;
            install(&msi, std::env::current_exe().ok().as_deref()).map_err(|e| format!("Windows Installer didn't start ({e})"))
        }),
        other_windows: Box::new(crate::single_instance::other_windows_open),
        at_start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::net::TcpListener;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn manifest(version: &str, file: &str, installer: &[u8]) -> Vec<u8> {
        serde_json::json!({
            "product": PRODUCT, "version": version, "file": file, "size": installer.len(),
            "sha256": hex(&Sha256::digest(installer)), "notes": "ลากแท็บไปรวมกับอีกหน้าต่างได้",
        })
        .to_string()
        .into_bytes()
    }

    fn signed(manifest: &[u8]) -> String {
        hex(&key().sign(manifest).to_bytes())
    }

    /// A web server on this machine answering each path with its body, until the test ends.
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
        let Ok(listener) = TcpListener::bind("127.0.0.1:0") else { panic!("no local port") };
        let Ok(address) = listener.local_addr() else { panic!("no local address") };
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut request = [0u8; 2048];
                let n = stream.read(&mut request).unwrap_or(0);
                let line = String::from_utf8_lossy(request.get(..n).unwrap_or_default()).into_owned();
                let path = line.split(' ').nth(1).unwrap_or_default().to_owned();
                let reply = match files.iter().find(|(p, _)| path == format!("/u/{p}")) {
                    Some((_, body)) => {
                        [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes(), body.clone()]
                            .concat()
                    }
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&reply);
            }
        });
        format!("http://{address}/u/")
    }

    #[test]
    fn versions_are_three_numbers_compared_as_numbers() {
        assert_eq!(Version::parse("0.18.42"), Some(Version(0, 18, 42)));
        assert!(Version::parse("0.18.10") > Version::parse("0.18.9"));
        assert!(Version::parse("0.19.0") > Version::parse("0.18.999"));
        for bad in ["", "0.18", "0.18.1.2", "0.18.x", "0.18.-1", "0.18.1-rc", " 0.18.1", "0..1", "99999999999.0.0"] {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
        assert_eq!(Version(0, 18, 42).to_string(), "0.18.42");
    }

    #[test]
    fn only_a_manifest_signed_with_the_release_key_is_believed() {
        let manifest = manifest("0.18.42", "bmu-pdf-0.18.42-windows-x64.msi", b"msi");
        let release = verify(&manifest, &signed(&manifest), &key().verifying_key());
        assert!(release.as_ref().is_ok_and(|r| r.version == Version(0, 18, 42) && r.size == 3 && r.notes.contains("แท็บ")), "{release:?}");

        let mut tampered = manifest.clone();
        if let Some(byte) = tampered.iter_mut().find(|b| **b == b'4') {
            *byte = b'9';
        }
        assert!(verify(&tampered, &signed(&manifest), &key().verifying_key()).is_err(), "a changed manifest");
        let stranger = SigningKey::from_bytes(&[8; 32]);
        assert!(verify(&manifest, &hex(&stranger.sign(&manifest).to_bytes()), &key().verifying_key()).is_err(), "another key");
        assert!(verify(&manifest, "", &key().verifying_key()).is_err(), "no signature");
        assert!(verify(&manifest, "zz", &key().verifying_key()).is_err(), "not hex");
    }

    #[test]
    fn a_signed_manifest_must_name_a_plain_installer_for_this_product() {
        for file in ["../evil.msi", r"..\evil.msi", "C:evil.msi", "setup.exe", ".msi", "a b.msi", "x/y.msi"] {
            let manifest = manifest("0.18.42", file, b"msi");
            assert!(verify(&manifest, &signed(&manifest), &key().verifying_key()).is_err(), "{file}");
        }
        let other = serde_json::json!({"product": "bmu-drive", "version": "9.0.0", "file": "a.msi", "size": 1, "sha256": hex(&[0; 32])})
            .to_string()
            .into_bytes();
        assert!(verify(&other, &signed(&other), &key().verifying_key()).is_err(), "another product");
        let huge = serde_json::json!({"product": PRODUCT, "version": "9.0.0", "file": "a.msi", "size": INSTALLER_LIMIT + 1, "sha256": hex(&[0; 32])})
            .to_string()
            .into_bytes();
        assert!(verify(&huge, &signed(&huge), &key().verifying_key()).is_err(), "too large");
    }

    #[test]
    fn a_newer_release_is_offered_from_the_first_source_that_answers() {
        let manifest = manifest("0.18.42", "bmu-pdf-0.18.42-windows-x64.msi", b"msi");
        let good = serve(vec![("update.json", manifest.clone()), ("update.json.sig", signed(&manifest).into_bytes())]);
        let empty = serve(vec![]);
        let key = key().verifying_key();
        let found = check_at(&[&empty, &good], &key, Version(0, 18, 7));
        assert!(found.as_ref().is_ok_and(|r| r.as_ref().is_some_and(|r| r.source == good)), "{found:?}");
        assert_eq!(check_at(&[&good], &key, Version(0, 18, 42)), Ok(None), "the same version");
        assert_eq!(check_at(&[&good], &key, Version(0, 19, 0)), Ok(None), "an older one");
        assert!(check_at(&[&empty], &key, Version(0, 18, 7)).is_err(), "nothing there");

        let forged = serve(vec![("update.json", manifest.clone()), ("update.json.sig", hex(&[1; 64]).into_bytes())]);
        assert!(check_at(&[&forged], &key, Version(0, 18, 7)).is_err(), "a forged signature");
    }

    #[test]
    fn the_installer_is_kept_only_when_it_matches_the_manifest() {
        let dir = std::env::temp_dir().join(format!("bmu-pdf-update-test-{}", std::process::id()));
        let installer = b"the real installer".to_vec();
        let manifest = manifest("0.18.42", "bmu-pdf-0.18.42-windows-x64.msi", &installer);
        let Ok(release) = verify(&manifest, &signed(&manifest), &key().verifying_key()) else { panic!("verified") };

        let good = Release { source: serve(vec![("bmu-pdf-0.18.42-windows-x64.msi", installer.clone())]), ..release.clone() };
        let seen = std::cell::Cell::new(0);
        let path = download(&good, &dir, &|done, total| {
            assert_eq!(total, 18);
            seen.set(done);
        });
        assert_eq!(path.as_ref().map(|p| std::fs::read(p).ok()), Ok(Some(installer.clone())));
        assert_eq!(seen.get(), 18);

        for body in [b"the fake installer".to_vec(), b"short".to_vec(), [installer.clone(), b"!".to_vec()].concat()] {
            let _ = std::fs::remove_dir_all(&dir);
            let bad = Release { source: serve(vec![("bmu-pdf-0.18.42-windows-x64.msi", body.clone())]), ..release.clone() };
            assert!(download(&bad, &dir, &|_, _| ()).is_err(), "{:?}", String::from_utf8_lossy(&body));
            assert_eq!(std::fs::read_dir(&dir).map(Iterator::count).ok(), Some(0), "nothing left behind");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
