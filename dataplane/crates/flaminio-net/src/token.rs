//! Capability tokens (ARCHITECTURE §8.4, §15.2), as issued by Go's `pkg/captoken`.
//!
//! `"FLCT"`, version 1, then (big-endian) `u16` volume length and volume, `u64` epoch, `u16`
//! node length and node, `i64` issue time, and a 64-byte Ed25519 signature over all of it.

use ring::signature::{ED25519, UnparsedPublicKey};

const MAGIC: &[u8; 4] = b"FLCT";
const SIG: usize = 64;
/// Largest token accepted.
pub const MAX_LEN: usize = 4 + 1 + 2 + 255 + 8 + 2 + 255 + 8 + SIG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub volume: String,
    pub epoch: u64,
    pub node: String,
    pub issued_at: i64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("invalid capability token")]
    Invalid,
    #[error("public key: {0}")]
    Key(&'static str),
}

/// The metadata service's token-verification key.
#[derive(Debug, Clone)]
pub struct TokenKey(Vec<u8>);

impl TokenKey {
    /// From the raw 32-byte Ed25519 public key.
    pub fn from_raw(key: &[u8]) -> Result<Self, TokenError> {
        if key.len() != 32 {
            return Err(TokenError::Key("an Ed25519 key is 32 bytes"));
        }
        Ok(Self(key.to_vec()))
    }

    /// From a PEM `PUBLIC KEY` (SubjectPublicKeyInfo), as `flaminio-ca token-key` writes it.
    pub fn from_pem(pem: &str) -> Result<Self, TokenError> {
        use rustls_pki_types::SubjectPublicKeyInfoDer;
        use rustls_pki_types::pem::PemObject;
        let spki = SubjectPublicKeyInfoDer::from_pem_slice(pem.as_bytes())
            .map_err(|_| TokenError::Key("no PEM public key"))?;
        // The DER prefix of an Ed25519 SubjectPublicKeyInfo (RFC 8410), then the key.
        const PREFIX: [u8; 12] = [
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        let der: &[u8] = spki.as_ref();
        match der.strip_prefix(&PREFIX[..]) {
            Some(raw) => Self::from_raw(raw),
            None => Err(TokenError::Key("not an Ed25519 public key")),
        }
    }

    /// Checks the signature and decodes the claims. The epoch and node are for the caller to
    /// check: they depend on replica state and on the peer's certificate.
    pub fn verify(&self, tok: &[u8]) -> Result<Claims, TokenError> {
        if tok.len() > MAX_LEN || tok.len() < SIG {
            return Err(TokenError::Invalid);
        }
        let (body, sig) = tok.split_at(tok.len() - SIG);
        UnparsedPublicKey::new(&ED25519, &self.0)
            .verify(body, sig)
            .map_err(|_| TokenError::Invalid)?;
        let mut r = body;
        let mut take = |n: usize| -> Result<&[u8], TokenError> {
            if r.len() < n {
                return Err(TokenError::Invalid);
            }
            let (a, b) = r.split_at(n);
            r = b;
            Ok(a)
        };
        if take(4)? != MAGIC || take(1)?[0] != 1 {
            return Err(TokenError::Invalid);
        }
        let n = u16::from_be_bytes(take(2)?.try_into().expect("2")) as usize;
        let volume = std::str::from_utf8(take(n)?)
            .map_err(|_| TokenError::Invalid)?
            .to_string();
        let epoch = u64::from_be_bytes(take(8)?.try_into().expect("8"));
        let n = u16::from_be_bytes(take(2)?.try_into().expect("2")) as usize;
        let node = std::str::from_utf8(take(n)?)
            .map_err(|_| TokenError::Invalid)?
            .to_string();
        let issued_at = i64::from_be_bytes(take(8)?.try_into().expect("8"));
        if !r.is_empty() || volume.is_empty() || node.is_empty() {
            return Err(TokenError::Invalid);
        }
        Ok(Claims {
            volume,
            epoch,
            node,
            issued_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// Same seed as Go's `pkg/captoken` tests. Vectors are built here so signed tokens are
    /// never stored in the repository (GitGuardian).
    const SEED: [u8; 32] = [7; 32];

    fn sign(kp: &Ed25519KeyPair, volume: &str, epoch: u64, node: &str, issued_at: i64) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(MAGIC);
        body.push(1);
        body.extend_from_slice(&(volume.len() as u16).to_be_bytes());
        body.extend_from_slice(volume.as_bytes());
        body.extend_from_slice(&epoch.to_be_bytes());
        body.extend_from_slice(&(node.len() as u16).to_be_bytes());
        body.extend_from_slice(node.as_bytes());
        body.extend_from_slice(&(issued_at as u64).to_be_bytes());
        let sig = kp.sign(&body);
        body.extend_from_slice(sig.as_ref());
        body
    }

    #[test]
    fn go_compatible_vectors() {
        let kp = Ed25519KeyPair::from_seed_unchecked(&SEED).unwrap();
        let key = TokenKey::from_raw(kp.public_key().as_ref()).unwrap();
        let good = sign(&kp, "pvc-0b1c", 7, "fl-b", 1791500000);
        let c = key.verify(&good).unwrap();
        assert_eq!(c.volume, "pvc-0b1c");
        assert_eq!(c.epoch, 7);
        assert_eq!(c.node, "fl-b");
        assert_eq!(c.issued_at, 1791500000);

        let long_vol = "v".repeat(255);
        let long = sign(&kp, &long_vol, u64::MAX, "n", -1);
        let c = key.verify(&long).unwrap();
        assert_eq!(c.volume, long_vol);
        assert_eq!(c.epoch, u64::MAX);
        assert_eq!(c.node, "n");
        assert_eq!(c.issued_at, -1);

        let mut flipped = good.clone();
        flipped[10] ^= 1;
        assert_eq!(key.verify(&flipped), Err(TokenError::Invalid));
        assert_eq!(
            key.verify(&good[..good.len() - 1]),
            Err(TokenError::Invalid)
        );
        assert_eq!(key.verify(&[]), Err(TokenError::Invalid));
        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(key.verify(&trailing), Err(TokenError::Invalid));
    }

    #[test]
    fn pem_key() {
        let kp = Ed25519KeyPair::from_seed_unchecked(&SEED).unwrap();
        let raw = kp.public_key().as_ref();
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(raw);
        fn b64_std(data: &[u8]) -> String {
            const T: &[u8] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for chunk in data.chunks(3) {
                let a = u32::from(chunk[0]);
                let b = u32::from(chunk.get(1).copied().unwrap_or(0));
                let c = u32::from(chunk.get(2).copied().unwrap_or(0));
                let n = (a << 16) | (b << 8) | c;
                s.push(T[(n >> 18) as usize] as char);
                s.push(T[((n >> 12) & 63) as usize] as char);
                s.push(if chunk.len() > 1 {
                    T[((n >> 6) & 63) as usize] as char
                } else {
                    '='
                });
                s.push(if chunk.len() > 2 {
                    T[(n & 63) as usize] as char
                } else {
                    '='
                });
            }
            s
        }
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            b64_std(&der)
        );
        let k = TokenKey::from_pem(&pem).unwrap();
        assert_eq!(k.0, raw);
        assert!(TokenKey::from_pem("nonsense").is_err());
    }
}
