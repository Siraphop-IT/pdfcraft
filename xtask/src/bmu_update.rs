//! `cargo xtask bmu-update`: the release key and signed manifest for BMU PDF's updates from the
//! office NAS (apps/pdfcraft/src/update.rs).
//!
//! - `keygen --key FILE` makes the release key (hex, never committed) and prints the public half
//!   to paste into `update.rs`. It refuses to replace an existing key: a lost or replaced key means
//!   every installed copy stops accepting updates.
//! - `sign --key FILE --msi FILE --out DIR [--notes TEXT | --notes-file FILE] [--version X.Y.Z]`
//!   writes `update.json` (product, version, installer name, size and SHA-256, notes) and
//!   `update.json.sig` (the Ed25519 signature of those exact bytes, hex) into DIR.
//! - `public --key FILE` prints the public half again.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

/// What the app accepts manifests for (`update.rs` PRODUCT).
const PRODUCT: &str = "bmu-pdf-windows-x64";

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let (command, rest) = args.split_first().context("usage: cargo xtask bmu-update keygen|sign|public --key FILE …")?;
    let option = |name: &str| rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).map(String::as_str);
    let key = PathBuf::from(option("--key").context("--key FILE is required")?);
    match command.as_str() {
        "keygen" => keygen(&key),
        "public" => {
            println!("{}", public_literal(&read_key(&key)?));
            Ok(())
        }
        "sign" => {
            let msi = PathBuf::from(option("--msi").context("--msi FILE is required")?);
            let out = PathBuf::from(option("--out").context("--out DIR is required")?);
            let notes = match (option("--notes"), option("--notes-file")) {
                (Some(text), _) => text.to_owned(),
                (None, Some(file)) => std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?,
                (None, None) => String::new(),
            };
            sign(&read_key(&key)?, &msi, option("--version"), notes.trim(), &out)
        }
        other => bail!("unknown bmu-update command `{other}` (keygen, sign, public)"),
    }
}

fn keygen(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        bail!("{} already exists; a new key would lock every installed copy out of updates", path.display());
    }
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, format!("{}\n", hex(&secret))).with_context(|| format!("writing {}", path.display()))?;
    println!("release key written to {} (keep a backup; never commit it)", path.display());
    println!("public key for apps/pdfcraft/src/update.rs:\n{}", public_literal(&SigningKey::from_bytes(&secret)));
    Ok(())
}

fn read_key(path: &Path) -> anyhow::Result<SigningKey> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let bytes = unhex(text.trim()).filter(|b| b.len() == 32).context("the key file is not 32 bytes of hex")?;
    let mut secret = [0u8; 32];
    secret.copy_from_slice(&bytes);
    Ok(SigningKey::from_bytes(&secret))
}

/// The manifest for `msi` and its signature, as written to `update.json` and `update.json.sig`.
pub fn manifest(key: &SigningKey, file: &str, installer: &[u8], version: &str, notes: &str) -> (String, String) {
    let json = serde_json::json!({
        "product": PRODUCT,
        "version": version,
        "file": file,
        "size": installer.len(),
        "sha256": hex(&Sha256::digest(installer)),
        "notes": notes,
    });
    let text = serde_json::to_string_pretty(&json).unwrap_or_default() + "\n";
    let signature = hex(&key.sign(text.as_bytes()).to_bytes()) + "\n";
    (text, signature)
}

fn sign(key: &SigningKey, msi: &Path, version: Option<&str>, notes: &str, out: &Path) -> anyhow::Result<()> {
    let file = msi.file_name().and_then(|n| n.to_str()).context("the installer has no file name")?;
    // bmu-pdf-0.18.42-windows-x64.msi → 0.18.42, unless --version says otherwise.
    let version = match version {
        Some(v) => v.to_owned(),
        None => file
            .strip_prefix("bmu-pdf-")
            .and_then(|r| r.strip_suffix("-windows-x64.msi"))
            .context("pass --version: it isn't in the file name")?
            .to_owned(),
    };
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        bail!("version {version} is not X.Y.Z");
    }
    let installer = std::fs::read(msi).with_context(|| format!("reading {}", msi.display()))?;
    let (text, signature) = manifest(key, file, &installer, &version, notes);
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    std::fs::write(out.join("update.json"), &text).context("writing update.json")?;
    std::fs::write(out.join("update.json.sig"), &signature).context("writing update.json.sig")?;
    println!("{}: BMU PDF {version}, {file}, {} bytes", out.display(), installer.len());
    Ok(())
}

fn public_literal(key: &SigningKey) -> String {
    let bytes: Vec<String> = key.verifying_key().to_bytes().iter().map(|b| format!("0x{b:02x}")).collect();
    format!("pub const PUBLIC_KEY: [u8; 32] = [{}];", bytes.join(", "))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes().chunks(2).map(|pair| std::str::from_utf8(pair).ok().and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn the_signature_covers_the_manifest_exactly_as_written() {
        let key = SigningKey::from_bytes(&[3; 32]);
        let (text, signature) = manifest(&key, "bmu-pdf-0.18.42-windows-x64.msi", b"msi", "0.18.42", "แก้ลากแท็บ");
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["product"], PRODUCT);
        assert_eq!(json["size"], 3);
        assert_eq!(json["sha256"], hex(&Sha256::digest(b"msi")));
        let bytes: [u8; 64] = unhex(signature.trim()).unwrap().try_into().unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&bytes);
        assert!(key.verifying_key().verify(text.as_bytes(), &signature).is_ok());
        assert!(key.verifying_key().verify(text.replace("0.18.42", "0.18.43").as_bytes(), &signature).is_err());
    }
}
