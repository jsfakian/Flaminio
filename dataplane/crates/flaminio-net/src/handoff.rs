//! Local target-start handoffs issued by an authorized metadata Attach, separate from the
//! unchanged FLCT credentials presented to replicas. No timestamp grants process freshness.

use ring::digest::{SHA256, digest};

use crate::token::{Claims, TokenKey};

/// Length of the unpredictable current-process challenge.
pub const NONCE_LEN: usize = 32;
/// The exact FLCH envelope length, including its Ed25519 signature.
pub const LEN: usize = 4 + 1 + 1 + NONCE_LEN + 32 + 64;
const BODY_LEN: usize = LEN - 64;

/// Handoff purposes have no interchangeable authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Normal = 1,
    /// Reserved for operation-specific private initialization. This proof alone cannot
    /// authorize initialization or replace its separate credentials.
    Initialization = 2,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandoffError {
    #[error("invalid target handoff")]
    Invalid,
}

/// A verified, current-process FLCH and its exact signed capability. Only signature verification
/// can construct this proof. Target admission must still match its claims to the local node and
/// current metadata, recheck its nonce/purpose, and enforce epoch/session lifetime fences.
///
/// Debug output deliberately omits the capability, envelope and process challenge.
#[derive(Clone)]
pub struct VerifiedHandoff {
    claims: Claims,
    capability: Vec<u8>,
    nonce: [u8; NONCE_LEN],
    purpose: Purpose,
}

impl std::fmt::Debug for VerifiedHandoff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedHandoff")
            .field("claims", &self.claims)
            .field("purpose", &self.purpose)
            .finish_non_exhaustive()
    }
}

impl VerifiedHandoff {
    pub fn claims(&self) -> &Claims {
        &self.claims
    }

    /// Use these bytes for replica sessions; they are the exact capability bound by FLCH.
    pub fn capability(&self) -> &[u8] {
        &self.capability
    }

    /// Recheck at target admission against the healthy daemon's own current process challenge.
    pub fn is_current(&self, nonce: &[u8; NONCE_LEN], purpose: Purpose) -> bool {
        self.nonce == *nonce && self.purpose == purpose
    }

    /// Match the verified FLCT to the current metadata epoch and local node. The metadata
    /// birth/epoch fence makes epochs unique across incarnations; the verified first epoch
    /// supplies the local replica's incarnation floor. A signature alone does not establish
    /// that this is still the volume's target.
    pub fn matches_target(&self, volume: &str, node: &str, epoch: u64) -> bool {
        self.claims.volume == volume && self.claims.node == node && self.claims.epoch == epoch
    }
}

impl TokenKey {
    /// Verify both signatures, exact FLCH encoding, exact capability digest and current
    /// process nonce/purpose. Strict target lifetimes reject legacy FLCT first_epoch=0.
    /// The returned opaque proof must be checked against metadata and node identity before
    /// target allocation or a replica Hello; see [`VerifiedHandoff::matches_target`].
    pub fn verify_handoff(
        &self,
        capability: &[u8],
        handoff: &[u8],
        nonce: &[u8; NONCE_LEN],
        purpose: Purpose,
    ) -> Result<VerifiedHandoff, HandoffError> {
        if handoff.len() != LEN || nonce.iter().all(|&b| b == 0) {
            return Err(HandoffError::Invalid);
        }
        let (body, signature) = handoff.split_at(BODY_LEN);
        if &body[..4] != b"FLCH"
            || body[4] != 1
            || body[5] != purpose as u8
            || body[6..6 + NONCE_LEN] != nonce[..]
        {
            return Err(HandoffError::Invalid);
        }
        self.verify_signature(body, signature)
            .map_err(|_| HandoffError::Invalid)?;
        let claims = self.verify(capability).map_err(|_| HandoffError::Invalid)?;
        if &body[6 + NONCE_LEN..] != digest(&SHA256, capability).as_ref() {
            return Err(HandoffError::Invalid);
        }
        if claims.first_epoch == 0 {
            return Err(HandoffError::Invalid);
        }
        Ok(VerifiedHandoff {
            claims,
            capability: capability.to_vec(),
            nonce: *nonce,
            purpose,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    fn token(signer: &Ed25519KeyPair, first: u64) -> Vec<u8> {
        let mut b = b"FLCT\x02\x00\x01v".to_vec();
        b.extend_from_slice(&7u64.to_be_bytes());
        b.extend_from_slice(b"\x00\x04fl-a");
        b.extend_from_slice(&0i64.to_be_bytes());
        b.extend_from_slice(&first.to_be_bytes());
        b.extend_from_slice(signer.sign(&b).as_ref());
        b
    }
    fn envelope(signer: &Ed25519KeyPair, token: &[u8], nonce: &[u8; 32], purpose: u8) -> Vec<u8> {
        let mut b = b"FLCH\x01".to_vec();
        b.push(purpose);
        b.extend_from_slice(nonce);
        b.extend_from_slice(digest(&SHA256, token).as_ref());
        b.extend_from_slice(signer.sign(&b).as_ref());
        b
    }
    #[test]
    fn both_signatures_exact_digest_nonce_purpose_and_nonlegacy_incarnation_are_required() {
        let signer = Ed25519KeyPair::from_seed_unchecked(&[83; 32]).unwrap();
        let key = TokenKey::from_raw(signer.public_key().as_ref()).unwrap();
        let nonce = [83; 32];
        let token = token(&signer, 1);
        let handoff = envelope(&signer, &token, &nonce, 1);
        assert_eq!(handoff.len(), 134);
        let proof = key
            .verify_handoff(&token, &handoff, &nonce, Purpose::Normal)
            .unwrap();
        assert!(proof.matches_target("v", "fl-a", 7));
        assert!(!proof.matches_target("v", "fl-b", 7));
        assert!(!proof.matches_target("v", "fl-a", 8));
        assert!(!proof.matches_target("other", "fl-a", 7));
        assert!(!proof.is_current(&[84; 32], Purpose::Normal));
        assert!(!proof.is_current(&nonce, Purpose::Initialization));
        for length in [0, 1, 69, 70, 133, 135, 4096] {
            let mut b = handoff.clone();
            b.resize(length, 0);
            assert!(
                key.verify_handoff(&token, &b, &nonce, Purpose::Normal)
                    .is_err()
            );
        }
        for byte in [0, 4, 5, 6, 37, 38, 69, 70, 133] {
            let mut b = handoff.clone();
            b[byte] ^= 1;
            assert!(
                key.verify_handoff(&token, &b, &nonce, Purpose::Normal)
                    .is_err()
            );
        }
        let mut b = handoff.clone();
        b[38] ^= 1;
        let signature = signer.sign(&b[..70]);
        b[70..].copy_from_slice(signature.as_ref());
        assert!(
            key.verify_handoff(&token, &b, &nonce, Purpose::Normal)
                .is_err(),
            "valid FLCH signature cannot excuse wrong capability digest"
        );
        let mut broken = token.clone();
        *broken.last_mut().unwrap() ^= 1;
        let b = envelope(&signer, &broken, &nonce, 1);
        assert!(
            key.verify_handoff(&broken, &b, &nonce, Purpose::Normal)
                .is_err(),
            "FLCH cannot replace FLCT verification"
        );
        let legacy = {
            let mut b = token[..token.len() - 72].to_vec();
            b[4] = 1;
            b.extend_from_slice(signer.sign(&b).as_ref());
            b
        };
        let b = envelope(&signer, &legacy, &nonce, 1);
        assert!(key.verify(&legacy).is_ok());
        assert!(
            key.verify_handoff(&legacy, &b, &nonce, Purpose::Normal)
                .is_err()
        );
        for p in [0, 2, 3, 255] {
            let b = envelope(&signer, &token, &nonce, p);
            assert!(
                key.verify_handoff(&token, &b, &nonce, Purpose::Normal)
                    .is_err()
            );
        }
        assert!(
            key.verify_handoff(&token, &handoff, &[0; 32], Purpose::Normal)
                .is_err()
        );
        let debug = format!("{proof:?}");
        assert!(!debug.contains("capability:"));
        assert!(!debug.contains("nonce:"));
    }
}
