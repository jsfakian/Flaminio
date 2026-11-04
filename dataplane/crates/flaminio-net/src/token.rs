//! Capability tokens (ARCHITECTURE §8.4, §15.2), as issued by Go's `pkg/captoken`.
//!
//! `"FLCT"`, version 1 or 2, then (big-endian) `u16` volume length and volume, `u64` epoch, `u16`
//! node length and node, `i64` issue time, in version 2 only the `u64` first epoch of the volume
//! (above 0 and below the epoch: docs/issues/30), and a 64-byte Ed25519 signature over all of it.

use ring::signature::{ED25519, UnparsedPublicKey};

const MAGIC: &[u8; 4] = b"FLCT";
const SIG: usize = 64;
/// Largest token accepted.
pub const MAX_LEN: usize = 4 + 1 + 2 + 255 + 8 + 2 + 255 + 8 + 8 + SIG;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub volume: String,
    pub epoch: u64,
    pub node: String,
    pub issued_at: i64,
    /// The volume's first epoch, below `epoch`; 0 for a volume without one (a version 1 token).
    /// A replica fenced below it holds only what earlier volumes of the name left
    /// (docs/issues/30, S5).
    pub first_epoch: u64,
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
        parse(body)
    }
}

/// Decodes a token as [`TokenKey::verify`] does, without checking its signature. Only for the
/// target's own replica, which has no session to verify it in and takes the volume's first epoch
/// from the attach's token (docs/issues/30, S6). #15 replaces it with a verified check.
#[cfg(feature = "unverified")]
pub fn decode_unverified(tok: &[u8]) -> Result<Claims, TokenError> {
    if tok.len() > MAX_LEN || tok.len() < SIG {
        return Err(TokenError::Invalid);
    }
    parse(&tok[..tok.len() - SIG])
}

/// Parses a token's body, everything before the signature, with Go's rules.
fn parse(body: &[u8]) -> Result<Claims, TokenError> {
    let mut r = body;
    let mut take = |n: usize| -> Result<&[u8], TokenError> {
        if r.len() < n {
            return Err(TokenError::Invalid);
        }
        let (a, b) = r.split_at(n);
        r = b;
        Ok(a)
    };
    if take(4)? != MAGIC {
        return Err(TokenError::Invalid);
    }
    let version = take(1)?[0];
    if version != 1 && version != 2 {
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
    let mut first_epoch = 0;
    if version == 2 {
        first_epoch = u64::from_be_bytes(take(8)?.try_into().expect("8"));
        if first_epoch == 0 || first_epoch >= epoch {
            return Err(TokenError::Invalid);
        }
    }
    if !r.is_empty() || volume.is_empty() || node.is_empty() {
        return Err(TokenError::Invalid);
    }
    Ok(Claims {
        volume,
        epoch,
        node,
        issued_at,
        first_epoch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// Same seed as Go's `pkg/captoken` tests. Vectors are built here so signed tokens are
    /// never stored in the repository (GitGuardian).
    const SEED: [u8; 32] = [7; 32];

    fn sign(kp: &Ed25519KeyPair, volume: &str, epoch: u64, node: &str, issued_at: i64) -> Vec<u8> {
        signed(kp, body(1, volume, epoch, node, issued_at, None))
    }

    /// A token body as Go's `Sign` writes it: version 2 when `first` is given.
    fn body(
        version: u8,
        volume: &str,
        epoch: u64,
        node: &str,
        issued_at: i64,
        first: Option<u64>,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(MAGIC);
        body.push(version);
        body.extend_from_slice(&(volume.len() as u16).to_be_bytes());
        body.extend_from_slice(volume.as_bytes());
        body.extend_from_slice(&epoch.to_be_bytes());
        body.extend_from_slice(&(node.len() as u16).to_be_bytes());
        body.extend_from_slice(node.as_bytes());
        body.extend_from_slice(&(issued_at as u64).to_be_bytes());
        if let Some(first) = first {
            body.extend_from_slice(&first.to_be_bytes());
        }
        body
    }

    fn signed(kp: &Ed25519KeyPair, mut body: Vec<u8>) -> Vec<u8> {
        let sig = kp.sign(&body);
        body.extend_from_slice(sig.as_ref());
        body
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// The token bodies of Go's `TestBodies` (`pkg/captoken`), byte for byte: version 1 for a
    /// volume without a first epoch, version 2 with one (docs/issues/30).
    const BODY_V1: &str =
        "464c43540100087076632d3062316300000000000000070004666c2d62000000006ac81ee0";
    const BODY_V2: &str = "464c43540200087076632d3062316300066517289880080004666c2d62000000006ac81ee00006651728988000";

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
        assert_eq!(c.first_epoch, 0);

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
        assert_eq!(hex(&good[..good.len() - SIG]), BODY_V1);

        // Version 2: the first epoch, signed.
        let (epoch, first) = (1_800_000_000_000_008, 1_800_000_000_000_000);
        let v2 = signed(
            &kp,
            body(2, "pvc-0b1c", epoch, "fl-b", 1791500000, Some(first)),
        );
        assert_eq!(hex(&v2[..v2.len() - SIG]), BODY_V2);
        let c = key.verify(&v2).unwrap();
        assert_eq!(
            (c.volume.as_str(), c.epoch, c.node.as_str(), c.issued_at),
            ("pvc-0b1c", epoch, "fl-b", 1791500000)
        );
        assert_eq!(c.first_epoch, first);
        let longest = signed(
            &kp,
            body(2, &long_vol, u64::MAX, &long_vol, -1, Some(u64::MAX - 1)),
        );
        assert_eq!(longest.len(), MAX_LEN);
        assert_eq!(key.verify(&longest).unwrap().first_epoch, u64::MAX - 1);
        // A first epoch of 0, equal to the epoch or above it, or missing; an unknown version.
        for first in [0, 7, 8] {
            let tok = signed(&kp, body(2, "pvc-0b1c", 7, "fl-b", 1791500000, Some(first)));
            assert_eq!(key.verify(&tok), Err(TokenError::Invalid), "first {first}");
        }
        let tok = signed(&kp, body(2, "pvc-0b1c", 7, "fl-b", 1791500000, None));
        assert_eq!(key.verify(&tok), Err(TokenError::Invalid));
        let tok = signed(
            &kp,
            body(3, "pvc-0b1c", epoch, "fl-b", 1791500000, Some(first)),
        );
        assert_eq!(key.verify(&tok), Err(TokenError::Invalid));
    }

    /// The target's own replica decodes its attach token without the signature, with the same
    /// rules otherwise (docs/issues/30, S6).
    #[cfg(feature = "unverified")]
    #[test]
    fn decode_unverified() {
        let kp = Ed25519KeyPair::from_seed_unchecked(&SEED).unwrap();
        let key = TokenKey::from_raw(kp.public_key().as_ref()).unwrap();
        let mut tok = signed(&kp, body(2, "v", 21, "fl-a", 0, Some(20)));
        let n = tok.len();
        tok[n - 1] ^= 1;
        assert_eq!(key.verify(&tok), Err(TokenError::Invalid));
        let c = super::decode_unverified(&tok).unwrap();
        assert_eq!(
            (c.volume.as_str(), c.epoch, c.node.as_str(), c.first_epoch),
            ("v", 21, "fl-a", 20)
        );
        let bad = signed(&kp, body(2, "v", 21, "fl-a", 0, Some(21)));
        assert_eq!(super::decode_unverified(&bad), Err(TokenError::Invalid));
        assert_eq!(super::decode_unverified(&[]), Err(TokenError::Invalid));
        assert_eq!(
            super::decode_unverified(&tok[..n - 1]),
            Err(TokenError::Invalid)
        );
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
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
