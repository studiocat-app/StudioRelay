//! StudioCat license verification.
//!
//! A license is `SCL1.<payload>.<signature>`: `payload` is base64url (no
//! padding) JSON, `signature` is the base64url Ed25519 signature over the
//! ASCII bytes `SCL1.<payload>`. The signature is checked before the payload
//! is parsed. The StudioCat app and its tunnel verify licenses with their own
//! implementations; all of them share the same test vectors.

use std::collections::HashMap;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::Deserialize;

const PREFIX: &str = "SCL1.";

/// The fields the relay needs. Unknown fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct License {
    pub v: u32,
    pub kid: String,
    /// License id, used for revocation.
    pub lid: String,
    pub plan: String,
    pub features: Vec<String>,
    /// Salted hash of the subscriber's email; the hosted relay's tenant key.
    pub sub: String,
    pub iat: u64,
    pub nbf: u64,
    pub exp: u64,
}

impl License {
    pub fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseError {
    Malformed,
    UnknownKey,
    BadSignature,
    UnsupportedVersion,
    NotYetValid,
    Expired,
    Revoked,
}

/// Holds the public keys a build trusts, by key id.
pub struct Verifier {
    keys: HashMap<String, VerifyingKey>,
}

impl Verifier {
    pub fn new<'a>(
        keys: impl IntoIterator<Item = (&'a str, [u8; 32])>,
    ) -> Result<Self, LicenseError> {
        let keys = keys
            .into_iter()
            .map(|(kid, bytes)| {
                VerifyingKey::from_bytes(&bytes)
                    .map(|k| (kid.to_owned(), k))
                    .map_err(|_| LicenseError::Malformed)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { keys })
    }

    /// Verifies `token` at `now` (Unix seconds). `revoked` answers whether a
    /// license id is on the signed revocation list.
    pub fn verify(
        &self,
        token: &str,
        now: u64,
        revoked: impl Fn(&str) -> bool,
    ) -> Result<License, LicenseError> {
        let rest = token.strip_prefix(PREFIX).ok_or(LicenseError::Malformed)?;
        let (payload_b64, sig_b64) = rest.split_once('.').ok_or(LicenseError::Malformed)?;
        let payload = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| LicenseError::Malformed)?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| LicenseError::Malformed)?;
        let signature = Signature::from_slice(&sig_bytes).map_err(|_| LicenseError::Malformed)?;

        // The key id is needed before the signature can be checked; read only
        // that field, then verify, then parse the whole payload.
        #[derive(Deserialize)]
        struct Kid {
            kid: String,
        }
        let kid: Kid = serde_json::from_slice(&payload).map_err(|_| LicenseError::Malformed)?;
        let key = self.keys.get(&kid.kid).ok_or(LicenseError::UnknownKey)?;
        let signed = &token.as_bytes()[..PREFIX.len() + payload_b64.len()];
        key.verify(signed, &signature)
            .map_err(|_| LicenseError::BadSignature)?;

        let license: License =
            serde_json::from_slice(&payload).map_err(|_| LicenseError::Malformed)?;
        if license.v != 1 {
            return Err(LicenseError::UnsupportedVersion);
        }
        if now < license.nbf {
            return Err(LicenseError::NotYetValid);
        }
        if now >= license.exp {
            return Err(LicenseError::Expired);
        }
        if revoked(&license.lid) {
            return Err(LicenseError::Revoked);
        }
        Ok(license)
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const SEED: [u8; 32] = [7; 32];

    fn issue(payload: &str, key: &SigningKey) -> String {
        let body = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(payload));
        let sig = key.sign(body.as_bytes());
        format!("{body}.{}", URL_SAFE_NO_PAD.encode(sig.to_bytes()))
    }

    fn payload(exp: u64) -> String {
        format!(
            r#"{{"v":1,"kid":"test","lid":"lic_1","plan":"pro","features":["relay.hosted"],"sub":"h:abc","iat":100,"nbf":100,"exp":{exp},"extra":true}}"#
        )
    }

    fn verifier() -> Verifier {
        let key = SigningKey::from_bytes(&SEED);
        Verifier::new([("test", key.verifying_key().to_bytes())]).unwrap()
    }

    #[test]
    fn accepts_a_valid_license_and_ignores_unknown_fields() {
        let token = issue(&payload(1_000), &SigningKey::from_bytes(&SEED));
        let license = verifier().verify(&token, 500, |_| false).unwrap();
        assert!(license.has("relay.hosted"));
        assert_eq!(license.sub, "h:abc");
    }

    #[test]
    fn rejects_time_revocation_and_tampering() {
        let key = SigningKey::from_bytes(&SEED);
        let token = issue(&payload(1_000), &key);
        let v = verifier();
        assert_eq!(
            v.verify(&token, 50, |_| false),
            Err(LicenseError::NotYetValid)
        );
        assert_eq!(
            v.verify(&token, 1_000, |_| false),
            Err(LicenseError::Expired)
        );
        assert_eq!(
            v.verify(&token, 500, |lid| lid == "lic_1"),
            Err(LicenseError::Revoked)
        );

        let forged = issue(&payload(9_999), &SigningKey::from_bytes(&[8; 32]));
        assert_eq!(
            v.verify(&forged, 500, |_| false),
            Err(LicenseError::BadSignature)
        );

        let (head, sig) = token.rsplit_once('.').unwrap();
        let swapped = format!("{PREFIX}{}.{sig}", URL_SAFE_NO_PAD.encode(payload(9_999)));
        assert_ne!(swapped, format!("{head}.{sig}"));
        assert_eq!(
            v.verify(&swapped, 500, |_| false),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn rejects_unknown_keys_and_garbage() {
        let v = verifier();
        let other = issue(
            &payload(1_000).replace("\"test\"", "\"other\""),
            &SigningKey::from_bytes(&SEED),
        );
        assert_eq!(
            v.verify(&other, 500, |_| false),
            Err(LicenseError::UnknownKey)
        );
        assert_eq!(
            v.verify("SCL2.x.y", 500, |_| false),
            Err(LicenseError::Malformed)
        );
        assert_eq!(
            v.verify("SCL1.!!.??", 500, |_| false),
            Err(LicenseError::Malformed)
        );
    }
}
