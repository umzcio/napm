//! Proves the updater's signature scheme actually enforces integrity.
//!
//! The Tauri updater verifies each downloaded bundle with `minisign-verify`
//! against the public key baked into `tauri.conf.json` before installing. This
//! test exercises that exact crate with a real fixture signed by the napm
//! updater private key (`~/.napm/napm-updater.key`), confirming that:
//!   1. a valid signature verifies against the baked public key, and
//!   2. any tampering (to the payload or the signature) is rejected.
//!
//! If signature checking ever silently broke, an attacker-substituted bundle
//! would install; this test is the guard against that.

use minisign_verify::{PublicKey, Signature};

// The key material (line 2) of the public key baked into tauri.conf.json
// (`plugins.updater.pubkey`). Same key that ships in the app.
const PUBKEY_B64: &str = "RWR0guc+BKkfYBbgFjYy3x/NAWdmX0rmlnHxbpFAP98e6QiApAljN1+u";

// Exact bytes that were signed.
const MESSAGE: &[u8] = b"napm updater signature enforcement fixture\n";

// The signature file produced by `tauri signer sign` over MESSAGE with the
// napm updater private key.
const SIGNATURE: &str = "untrusted comment: signature from tauri secret key\n\
RUR0guc+BKkfYNpw93fqQRBmeyFavLEEL6O5bVoOC2edRNqrF/Id0Ep5TOQlCZoRcJjpDJpV+otej3aiALbPJyujF2/L0RpVaQw=\n\
trusted comment: timestamp:1780282103\tfile:napm-fixture.txt\n\
Pgh6w4QdtkftsnHijyGhtAAH3iJ/YS+H7vc2xLQKkonHRVf1iCWmqFg3t566Ggttpp1E9QLsj2jhptHYvKzzCQ==\n";

#[test]
fn valid_signature_verifies_against_baked_pubkey() {
    let pk = PublicKey::from_base64(PUBKEY_B64).expect("pubkey parses");
    let sig = Signature::decode(SIGNATURE).expect("signature parses");
    pk.verify(MESSAGE, &sig, false)
        .expect("the real signature must verify against the baked-in public key");
}

#[test]
fn tampered_payload_is_rejected() {
    let pk = PublicKey::from_base64(PUBKEY_B64).expect("pubkey parses");
    let sig = Signature::decode(SIGNATURE).expect("signature parses");
    let mut tampered = MESSAGE.to_vec();
    tampered[0] ^= 0x01; // flip one bit of the "payload"
    assert!(
        pk.verify(&tampered, &sig, false).is_err(),
        "a modified payload must NOT verify (substituted bundle would be rejected)"
    );
}

#[test]
fn tampered_signature_is_rejected() {
    let pk = PublicKey::from_base64(PUBKEY_B64).expect("pubkey parses");
    // Corrupt one base64 char of the signature line. Either decode fails or
    // verification fails; both mean a forged signature does not pass.
    let forged = SIGNATURE.replacen("RUR0guc", "RUR0guX", 1);
    let rejected = match Signature::decode(&forged) {
        Err(_) => true,
        Ok(sig) => pk.verify(MESSAGE, &sig, false).is_err(),
    };
    assert!(rejected, "a corrupted signature must NOT verify");
}
