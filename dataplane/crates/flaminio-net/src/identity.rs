//! Workload identities: SPIFFE IDs in certificates (same format as Go's `pkg/pki`).
//!
//! ```text
//! spiffe://<trust-domain>/flaminio/node/<node>/<component>
//! spiffe://<trust-domain>/flaminio/admin/<name>
//! ```

use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Node {
        trust_domain: String,
        node: String,
        component: String,
    },
    Admin {
        trust_domain: String,
        name: String,
    },
}

impl Identity {
    pub fn trust_domain(&self) -> &str {
        match self {
            Identity::Node { trust_domain, .. } | Identity::Admin { trust_domain, .. } => {
                trust_domain
            }
        }
    }

    /// The node, if this is a node identity with the given component.
    pub fn node_with(&self, want: &str) -> Option<&str> {
        match self {
            Identity::Node {
                node, component, ..
            } if component == want => Some(node),
            _ => None,
        }
    }
}

impl std::fmt::Display for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Identity::Node {
                trust_domain,
                node,
                component,
            } => write!(
                f,
                "spiffe://{trust_domain}/flaminio/node/{node}/{component}"
            ),
            Identity::Admin { trust_domain, name } => {
                write!(f, "spiffe://{trust_domain}/flaminio/admin/{name}")
            }
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("not a Flaminio SPIFFE ID: {0:?}")]
    BadId(String),
    #[error("certificate: {0}")]
    BadCert(String),
    #[error("certificate has {0} URI SANs, want 1")]
    UriCount(usize),
}

/// A path segment: what Go's pkg/pki accepts.
fn segment_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Parses a Flaminio SPIFFE ID.
pub fn parse_id(id: &str) -> Result<Identity, IdError> {
    let bad = || IdError::BadId(id.to_string());
    let rest = id.strip_prefix("spiffe://").ok_or_else(bad)?;
    let (td, path) = rest.split_once('/').ok_or_else(bad)?;
    if td.is_empty() || td.contains(['?', '#', '@', ':']) {
        return Err(bad());
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.iter().skip(1).any(|p| !segment_ok(p)) {
        return Err(bad());
    }
    match parts.as_slice() {
        ["flaminio", "node", node, component] => Ok(Identity::Node {
            trust_domain: td.to_string(),
            node: node.to_string(),
            component: component.to_string(),
        }),
        ["flaminio", "admin", name] => Ok(Identity::Admin {
            trust_domain: td.to_string(),
            name: name.to_string(),
        }),
        _ => Err(bad()),
    }
}

/// The identity in a certificate (DER): its single URI SAN.
pub fn from_cert(der: &[u8]) -> Result<Identity, IdError> {
    let (_, cert) = X509Certificate::from_der(der).map_err(|e| IdError::BadCert(e.to_string()))?;
    let uris: Vec<&str> = match cert
        .subject_alternative_name()
        .map_err(|e| IdError::BadCert(e.to_string()))?
    {
        Some(san) => san
            .value
            .general_names
            .iter()
            .filter_map(|n| match n {
                GeneralName::URI(u) => Some(*u),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    };
    if uris.len() != 1 {
        return Err(IdError::UriCount(uris.len()));
    }
    parse_id(uris[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_go() {
        assert_eq!(
            parse_id("spiffe://lab/flaminio/node/fl-a/io").unwrap(),
            Identity::Node {
                trust_domain: "lab".into(),
                node: "fl-a".into(),
                component: "io".into()
            }
        );
        assert_eq!(
            parse_id("spiffe://lab/flaminio/admin/ops")
                .unwrap()
                .to_string(),
            "spiffe://lab/flaminio/admin/ops"
        );
        for bad in [
            "https://lab/flaminio/node/a/io",
            "spiffe://lab/flaminio/node/a",
            "spiffe://lab/flaminio/node/../io",
            "spiffe://lab/other/node/a/io",
            "spiffe:///flaminio/node/a/io",
            "spiffe://lab/flaminio/node/a/io?x=1",
            "spiffe://lab/flaminio/node/a/io/",
            "spiffe://u@lab/flaminio/node/a/io",
        ] {
            assert!(parse_id(bad).is_err(), "accepted {bad}");
        }
    }
}
