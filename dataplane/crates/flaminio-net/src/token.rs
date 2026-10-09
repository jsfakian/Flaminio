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

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// The vectors written by Go's pkg/captoken.
    #[test]
    fn go_vectors() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../pkg/captoken/testdata/vectors.json"
        );
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let key = TokenKey::from_raw(&hex(v["public_key"].as_str().unwrap())).unwrap();
        let vectors = v["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 6);
        for t in vectors {
            let name = t["name"].as_str().unwrap();
            let got = key.verify(&hex(t["token"].as_str().unwrap()));
            if t["valid"].as_bool().unwrap() {
                let c = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(c.volume, t["volume"].as_str().unwrap(), "{name}");
                assert_eq!(c.epoch, t["epoch"].as_u64().unwrap(), "{name}");
                assert_eq!(c.node, t["node"].as_str().unwrap(), "{name}");
                assert_eq!(c.issued_at, t["issued_at"].as_i64().unwrap(), "{name}");
            } else {
                assert_eq!(got, Err(TokenError::Invalid), "{name}");
            }
        }
    }

    #[test]
    fn pem_key() {
        let pem = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA6kpsY+KcUgq+9VB7Ey7F+ZVHdq6+vnuSQh7qaRRG0iw=\n-----END PUBLIC KEY-----\n";
        let k = TokenKey::from_pem(pem).unwrap();
        assert_eq!(
            k.0,
            hex("ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c")
        );
        assert!(TokenKey::from_pem("nonsense").is_err());
    }
}
