//! Certification authorities BMU PDF trusts out of the box, so Thai government documents (DBD
//! company certificates, Revenue Department forms) validate without each office PC adding a
//! certificate by hand.
//!
//! - Thailand National Root Certification Authority - G1 (ETDA), the anchor. SHA-256
//!   2a8da2f8d23e0cd3b5871ecfb0f42276ca73230667f474eede71c5ee32cc3ec6, fetched from
//!   nrca.go.th over HTTP and HTTPS (identical) and matching crt.sh's record.
//! - Thai Digital ID CA G3, issued by that root. SHA-256
//!   981d016eb85502b6de8670598b2dd7a78525a391471f55542eea2a27d812e4bf, from thaidigitalid.com.
//!   Signed documents often carry only the signer's certificate, so the chain needs it to reach
//!   the root; it is offered for chain building only and is not itself an anchor.

use std::sync::LazyLock;

use super::Certificate;

/// The trusted roots.
static ANCHORS: LazyLock<Vec<Certificate>> = LazyLock::new(|| parse(&[include_bytes!("anchors/thailand-national-root-ca-g1.der")]));

/// Issuing CAs under those roots, for building chains the document leaves short.
static INTERMEDIATES: LazyLock<Vec<Certificate>> = LazyLock::new(|| parse(&[include_bytes!("anchors/thai-digital-id-ca-g3.der")]));

/// A certificate that fails to parse is left out, never a panic.
fn parse(raw: &[&[u8]]) -> Vec<Certificate> {
    raw.iter().filter_map(|der| Certificate::parse(der).ok()).collect()
}

pub(super) fn anchors() -> &'static [Certificate] {
    &ANCHORS
}

/// Every built-in certificate a chain may pass through.
pub(super) fn chain_certificates() -> impl Iterator<Item = &'static Certificate> {
    ANCHORS.iter().chain(INTERMEDIATES.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::TrustStore;
    use crate::x509::build_chain;

    #[test]
    fn the_built_in_certificates_all_parse() {
        assert_eq!(ANCHORS.len(), 1);
        assert_eq!(INTERMEDIATES.len(), 1);
        let sha256 = |c: &Certificate| c.fingerprint().replace(' ', "").to_ascii_lowercase();
        assert_eq!(sha256(&ANCHORS[0]), "2a8da2f8d23e0cd3b5871ecfb0f42276ca73230667f474eede71c5ee32cc3ec6");
        assert_eq!(sha256(&INTERMEDIATES[0]), "981d016eb85502b6de8670598b2dd7a78525a391471f55542eea2a27d812e4bf");
        assert!(ANCHORS[0].is_self_signed());
    }

    #[test]
    fn an_empty_store_still_trusts_thai_digital_id() {
        let store = TrustStore::default();
        let g3 = &INTERMEDIATES[0];
        assert!(!store.trusts(g3), "an issuing CA is not an anchor");
        let pool: Vec<Certificate> = store.chain_pool();
        let chain = build_chain(g3, &pool, None);
        assert_eq!(chain.len(), 2, "G3 chains to the national root");
        assert!(chain.iter().any(|c| store.trusts(c)), "the national root is trusted with no user certificates");
    }

    #[test]
    fn user_certificates_are_kept_apart_from_the_built_in_ones() {
        let store = TrustStore::default();
        assert!(store.certs.is_empty(), "the built-ins are not listed, saved or removable as the user's");
    }
}
