use anyhow::{Context, Result};
use axum::{
    extract::connect_info::Connected,
    serve::{IncomingStream, Listener},
};
use copilotmail_platform::NativeTlsSettings;
use rustls::{RootCertStore, ServerConfig, server::WebPkiClientVerifier};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write,
    io::{self, BufReader, Cursor},
    net::SocketAddr,
    sync::Arc,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, server::TlsStream};

const MAX_TLS_FILE_BYTES: usize = 1024 * 1024;

/// Connection evidence produced by the actual TCP/TLS listener.
///
/// The certificate fingerprint is populated only after rustls validates the
/// presented chain against the configured client CA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsConnectionInfo {
    pub remote_addr: SocketAddr,
    pub client_certificate_fingerprint: Option<String>,
}

impl Connected<IncomingStream<'_, TcpListener>> for TlsConnectionInfo {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self {
            remote_addr: *stream.remote_addr(),
            client_certificate_fingerprint: None,
        }
    }
}

impl Connected<IncomingStream<'_, NativeTlsListener>> for TlsConnectionInfo {
    fn connect_info(stream: IncomingStream<'_, NativeTlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

/// Axum listener that performs native TLS and optional verified client
/// authentication before exposing a connection to HTTP routing.
pub struct NativeTlsListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl NativeTlsListener {
    pub async fn bind(address: SocketAddr, settings: &NativeTlsSettings) -> Result<Self> {
        let certificate_pem = read_bounded(
            &settings.server_certificate_file,
            "native TLS server certificate",
        )
        .await?;
        let private_key_pem =
            read_bounded(&settings.server_private_key_file, "native TLS private key").await?;
        let client_ca_pem = read_bounded(&settings.client_ca_file, "native TLS client CA").await?;

        Self::from_pem(address, certificate_pem, private_key_pem, client_ca_pem).await
    }

    async fn from_pem(
        address: SocketAddr,
        certificate_pem: Vec<u8>,
        private_key_pem: Vec<u8>,
        client_ca_pem: Vec<u8>,
    ) -> Result<Self> {
        let certificates = rustls_pemfile::certs(&mut BufReader::new(Cursor::new(certificate_pem)))
            .collect::<Result<Vec<_>, _>>()
            .context("parse native TLS server certificate chain")?;
        anyhow::ensure!(
            !certificates.is_empty(),
            "native TLS server certificate chain is empty"
        );
        let private_key =
            rustls_pemfile::private_key(&mut BufReader::new(Cursor::new(private_key_pem)))
                .context("parse native TLS private key")?
                .context("native TLS private key is missing")?;
        let client_ca_certificates =
            rustls_pemfile::certs(&mut BufReader::new(Cursor::new(client_ca_pem)))
                .collect::<Result<Vec<_>, _>>()
                .context("parse native TLS client CA")?;
        anyhow::ensure!(
            !client_ca_certificates.is_empty(),
            "native TLS client CA is empty"
        );

        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut roots = RootCertStore::empty();
        for certificate in client_ca_certificates {
            roots
                .add(certificate)
                .context("add native TLS client CA trust anchor")?;
        }
        let verifier = WebPkiClientVerifier::builder(Arc::new(roots))
            .allow_unauthenticated()
            .build()
            .context("build native TLS client certificate verifier")?;
        let mut server_config = ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(certificates, private_key)
            .context("build native TLS server configuration")?;
        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        Ok(Self {
            listener: TcpListener::bind(address)
                .await
                .context("bind native TLS listener")?,
            acceptor: TlsAcceptor::from(Arc::new(server_config)),
        })
    }
}

impl Listener for NativeTlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = TlsConnectionInfo;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, remote_addr) = match self.listener.accept().await {
                Ok(connection) => connection,
                Err(error) => {
                    tracing::warn!(%error, "native TLS TCP accept failed");
                    continue;
                }
            };
            match self.acceptor.accept(stream).await {
                Ok(stream) => {
                    let fingerprint = stream
                        .get_ref()
                        .1
                        .peer_certificates()
                        .and_then(|chain| chain.first())
                        .map(|certificate| sha256_hex(certificate.as_ref()));
                    return (
                        stream,
                        TlsConnectionInfo {
                            remote_addr,
                            client_certificate_fingerprint: fingerprint,
                        },
                    );
                }
                Err(error) => {
                    tracing::warn!(%remote_addr, %error, "native TLS handshake rejected");
                }
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener
            .local_addr()
            .map(|remote_addr| TlsConnectionInfo {
                remote_addr,
                client_certificate_fingerprint: None,
            })
    }
}

async fn read_bounded(path: &std::path::Path, label: &str) -> Result<Vec<u8>> {
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("inspect {label} file {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= MAX_TLS_FILE_BYTES as u64,
        "{label} file is invalid or too large"
    );
    tokio::fs::read(path)
        .await
        .with_context(|| format!("read {label} file {}", path.display()))
}

fn sha256_hex(value: &[u8]) -> String {
    let mut fingerprint = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to String cannot fail");
    }
    fingerprint
}

/// Parses the first X.509 PEM certificate and returns the same canonical
/// fingerprint used by the native TLS listener.
pub fn certificate_fingerprint_from_pem(value: &[u8]) -> Result<String> {
    let certificates = rustls_pemfile::certs(&mut BufReader::new(Cursor::new(value)))
        .collect::<Result<Vec<_>, _>>()
        .context("parse Agent mTLS certificate")?;
    let leaf = certificates
        .first()
        .context("Agent mTLS certificate is missing")?;
    Ok(sha256_hex(leaf.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
        KeyPair, KeyUsagePurpose,
    };
    use rustls::{
        ClientConfig,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    };
    use tokio_rustls::TlsConnector;

    #[test]
    fn certificate_fingerprint_is_stable_lowercase_sha256() {
        assert_eq!(
            sha256_hex(b"certificate"),
            "03d66dd08835c1ca3f128cceacd1f31ac94163096b20f445ae84285bc0832d72"
        );
    }

    #[tokio::test]
    async fn native_tls_verifies_and_exposes_client_leaf_certificate() {
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();

        let server_key = KeyPair::generate().unwrap();
        let mut server_params = CertificateParams::new(vec!["localhost".into()]).unwrap();
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_certificate = server_params.signed_by(&server_key, &ca).unwrap();

        let client_key = KeyPair::generate().unwrap();
        let mut client_params = CertificateParams::new(vec!["agent.test".into()]).unwrap();
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_certificate = client_params.signed_by(&client_key, &ca).unwrap();
        let expected_fingerprint = sha256_hex(client_certificate.der().as_ref());

        let mut listener = NativeTlsListener::from_pem(
            "127.0.0.1:0".parse().unwrap(),
            server_certificate.pem().into_bytes(),
            server_key.serialize_pem().into_bytes(),
            ca.pem().into_bytes(),
        )
        .await
        .unwrap();
        let address = listener.listener.local_addr().unwrap();
        let accept = tokio::spawn(async move { listener.accept().await.1 });

        let mut roots = RootCertStore::empty();
        roots.add(ca.der().clone()).unwrap();
        let client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(
                vec![client_certificate.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(client_key.serialize_der())),
            )
            .unwrap();
        let tcp = TcpStream::connect(address).await.unwrap();
        TlsConnector::from(Arc::new(client_config))
            .connect(ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap();
        let info = accept.await.unwrap();
        assert_eq!(
            info.client_certificate_fingerprint,
            Some(expected_fingerprint)
        );
    }
}
