//! Manifest signatures: a fixture signed with a throwaway key (tests/fixtures/test.pub).

use rigbrew_core::manifest::{verify_signature, Manifest, ManifestError};

const MANIFEST: &str = include_str!("fixtures/manifest.json");
const SIG: &str = include_str!("fixtures/manifest.json.minisig");

fn test_key() -> String {
    include_str!("fixtures/test.pub").lines().nth(1).unwrap().trim().to_string()
}

#[test]
fn a_valid_signature_passes() {
    verify_signature(MANIFEST.as_bytes(), SIG, &test_key()).unwrap();
}

#[test]
fn one_changed_byte_fails() {
    let tampered = MANIFEST.replace("example.invalid", "example.invaliD");
    assert!(matches!(verify_signature(tampered.as_bytes(), SIG, &test_key()), Err(ManifestError::BadSignature(_))));
}

#[test]
fn another_key_fails() {
    // Rigbrew's real key did not sign the fixture.
    assert!(matches!(Manifest::parse_signed(MANIFEST, SIG), Err(ManifestError::BadSignature(_))));
}

#[test]
fn garbage_signatures_fail_cleanly() {
    for sig in ["", "not a signature", "untrusted comment: x\nAAAA\ntrusted comment: y\nBBBB\n"] {
        assert!(matches!(verify_signature(MANIFEST.as_bytes(), sig, &test_key()), Err(ManifestError::BadSignature(_))), "{sig:?}");
    }
}
