//! TLS 1.3 configurations with client certificates required both ways, and the peer's
//! identity.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

use crate::identity::{self, IdError, Identity};

/// A certificate chain, its key, and the CA bundle that verifies peers.
#[derive(Debug, Clone)]
pub struct Files {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub ca: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("{0}: {1}")]
    Pem(PathBuf, String),
    #[error("TLS configuration: {0}")]
    Config(#[from] rustls::Error),
    #[error("client verifier: {0}")]
    Verifier(String),
}

struct Loaded {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    roots: Arc<RootCertStore>,
}

fn certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let v: Result<Vec<_>, _> = CertificateDer::pem_file_iter(path)
        .map_err(|e| TlsError::Pem(path.into(), e.to_string()))?
        .collect();
    let v = v.map_err(|e| TlsError::Pem(path.into(), e.to_string()))?;
    if v.is_empty() {
        return Err(TlsError::Pem(path.into(), "no certificates".into()));
    }
    Ok(v)
}

impl Files {
    fn load(&self) -> Result<Loaded, TlsError> {
        let chain = certs(&self.cert)?;
        let key = PrivateKeyDer::from_pem_file(&self.key)
            .map_err(|e| TlsError::Pem(self.key.clone(), e.to_string()))?;
        let mut roots = RootCertStore::empty();
        for c in certs(&self.ca)? {
            roots
                .add(c)
                .map_err(|e| TlsError::Pem(self.ca.clone(), e.to_string()))?;
        }
        Ok(Loaded {
            chain,
            key,
            roots: Arc::new(roots),
        })
    }

    /// Server side: TLS 1.3 only, client certificates required and verified.
    pub fn server_config(&self) -> Result<Arc<ServerConfig>, TlsError> {
        let l = self.load()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = WebPkiClientVerifier::builder_with_provider(l.roots, provider.clone())
            .build()
            .map_err(|e| TlsError::Verifier(e.to_string()))?;
        let cfg = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(l.chain, l.key)?;
        Ok(Arc::new(cfg))
    }

    /// Client side: TLS 1.3 only, presenting our certificate.
    pub fn client_config(&self) -> Result<Arc<ClientConfig>, TlsError> {
        let l = self.load()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(l.roots)
            .with_client_auth_cert(l.chain, l.key)?;
        Ok(Arc::new(cfg))
    }
}

/// The identity of a peer whose certificate chain the handshake verified, in `trust_domain`.
pub fn peer_identity(
    certs: Option<&[CertificateDer<'_>]>,
    trust_domain: &str,
) -> Result<Identity, IdError> {
    let leaf = certs
        .and_then(|c| c.first())
        .ok_or_else(|| IdError::BadCert("no peer certificate".into()))?;
    let id = identity::from_cert(leaf.as_ref())?;
    if id.trust_domain() != trust_domain {
        return Err(IdError::BadId(id.to_string()));
    }
    Ok(id)
}
