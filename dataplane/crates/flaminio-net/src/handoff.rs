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
