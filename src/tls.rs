// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

//! TLS for the OpenWire listener: the certificate, the accepted versions and the client
//! certificate rules, all checked at startup and by `check-config` so a broken file is a
//! configuration error and never a surprise at the first connection.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};

use crate::config::Config;

/// A JKS keystore starts with this magic; it is refused with a conversion hint.
const JKS_MAGIC: [u8; 4] = [0xfe, 0xed, 0xfe, 0xed];
/// A certificate expiring within this many days is worth a warning.
const EXPIRY_WARNING_DAYS: i64 = 30;

/// What a TLS connection negotiated, for the admin console.
#[derive(Debug, Clone, Default)]
pub struct Negotiated {
    pub version: String,
    pub cipher: String,
    /// Subject of the client certificate, when one was verified.
    pub client_cert: Option<String>,
}

/// The listener's configuration and what the operator should know about it.
#[derive(Debug)]
pub struct Prepared {
    pub config: Arc<ServerConfig>,
    /// Certificate problems that do not stop the broker, such as an expiry close by.
    pub warnings: Vec<String>,
}

fn error(key: &str, path: &Path, what: &str) -> String {
    format!("configuration error: {key} = \"{}\": {what}", path.display())
}

fn read(key: &str, path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| error(key, path, &format!("cannot read it: {e}")))
}

/// Loads the certificate chain and key from PEM files.
fn pem_pair(cert: &Path, key: &Path) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), String> {
    let cert_bytes = read("tls.cert", cert)?;
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<Result<_, _>>()
        .map_err(|e| error("tls.cert", cert, &format!("is not a PEM certificate: {e}")))?;
    if chain.is_empty() {
        return Err(error("tls.cert", cert, "holds no certificate"));
    }
    let key_bytes = read("tls.key", key)?;
    let private = PrivateKeyDer::from_pem_slice(&key_bytes).map_err(|e| {
        error(
            "tls.key",
            key,
            &format!("holds no usable private key (PKCS#8, PKCS#1 or SEC1, unencrypted): {e}"),
        )
    })?;
    Ok((chain, private))
}

/// Loads the certificate chain and key from a PKCS#12 keystore.
fn pkcs12_pair(store: &Path, password: &str) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), String> {
    let data = read("tls.keystore", store)?;
    if data.starts_with(&JKS_MAGIC) {
        return Err(error(
            "tls.keystore",
            store,
            "is a JKS keystore, which is not read; convert it with \
             `keytool -importkeystore -srckeystore <file> -destkeystore <file>.p12 -deststoretype PKCS12`",
        ));
    }
    let keystore = p12_keystore::KeyStore::from_pkcs12(&data, password, Default::default())
        .map_err(|e| error("tls.keystore", store, &format!("cannot be read: {e}")))?;
    let chains: Vec<&p12_keystore::PrivateKeyChain> = keystore
        .entries()
        .filter_map(|(_, entry)| match entry {
            p12_keystore::KeyStoreEntry::PrivateKeyChain(chain) => Some(chain),
            _ => None,
        })
        .collect();
    let chain = match chains.as_slice() {
        [one] => *one,
        [] => return Err(error("tls.keystore", store, "holds no private key entry")),
        many => {
            return Err(error(
                "tls.keystore",
                store,
                &format!("holds {} private key entries; it must hold exactly one", many.len()),
            ))
        }
    };
    if chain.certs().is_empty() {
        return Err(error("tls.keystore", store, "holds a key without its certificate"));
    }
    let certs = chain
        .certs()
        .iter()
        .map(|c| CertificateDer::from(c.as_der().to_vec()))
        .collect();
    let key = PrivateKeyDer::try_from(chain.key().as_der().to_vec())
        .map_err(|e| error("tls.keystore", store, &format!("holds an unusable private key: {e}")))?;
    Ok((certs, key))
}

/// The trusted CA certificates a client certificate must chain to.
fn client_roots(path: &Path) -> Result<RootCertStore, String> {
    let bytes = read("tls.client_ca", path)?;
    let mut roots = RootCertStore::empty();
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<_, _>>()
        .map_err(|e| error("tls.client_ca", path, &format!("is not a PEM bundle: {e}")))?;
    if certs.is_empty() {
        return Err(error("tls.client_ca", path, "holds no certificate"));
    }
    for cert in certs {
        roots.add(cert).map_err(|e| {
            error(
                "tls.client_ca",
                path,
                &format!("holds a certificate that is not usable: {e}"),
            )
        })?;
    }
    Ok(roots)
}

/// The subject of a certificate, as `CN=broker, O=Example`.
pub fn subject(der: &[u8]) -> Option<String> {
    x509_parser::parse_x509_certificate(der)
        .ok()
        .map(|(_, cert)| cert.subject().to_string())
}

/// A warning when the leaf certificate has expired or is about to.
fn expiry_warning(leaf: &CertificateDer<'_>) -> Option<String> {
    let (_, cert) = x509_parser::parse_x509_certificate(leaf.as_ref()).ok()?;
    let end = cert.validity().not_after;
    let left = end.timestamp() - chrono::Local::now().timestamp();
    let date = chrono::DateTime::from_timestamp(end.timestamp(), 0)?
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S");
    if left <= 0 {
        Some(format!("the TLS certificate expired on {date}"))
    } else if left < EXPIRY_WARNING_DAYS * 86_400 {
        Some(format!(
            "the TLS certificate expires on {date}, in {} day(s)",
            left / 86_400
        ))
    } else {
        None
    }
}

/// Installs the crypto provider once per process, so building a config never panics.
fn provider() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Reads and checks everything `[tls]` points at, and builds the listener's configuration.
/// Every error names the key and the file, so `check-config` can report it as it is.
pub fn prepare(cfg: &Config) -> Result<Prepared, String> {
    provider();
    let (chain, key) = match (&cfg.tls_cert, &cfg.tls_key, &cfg.tls_keystore) {
        (Some(cert), Some(key), None) => pem_pair(cert, key)?,
        (None, None, Some(store)) => pkcs12_pair(store, &cfg.tls_keystore_password)?,
        _ => return Err("configuration error: tls.enabled needs either tls.cert and tls.key, or tls.keystore".into()),
    };
    let warnings = chain.first().and_then(expiry_warning).into_iter().collect();
    let versions: &[&rustls::SupportedProtocolVersion] = if cfg.tls_min_version == "1.3" {
        &[&rustls::version::TLS13]
    } else {
        &[&rustls::version::TLS12, &rustls::version::TLS13]
    };
    let builder = ServerConfig::builder_with_protocol_versions(versions);
    let builder = match cfg.tls_client_auth.as_str() {
        "none" => builder.with_no_client_auth(),
        mode => {
            let ca = cfg
                .tls_client_ca
                .as_ref()
                .ok_or("configuration error: tls.client_ca is required with tls.client_auth")?;
            let mut verifier = WebPkiClientVerifier::builder(Arc::new(client_roots(ca)?));
            if mode == "optional" {
                verifier = verifier.allow_unauthenticated();
            }
            let verifier = verifier
                .build()
                .map_err(|e| format!("configuration error: tls.client_ca cannot be used: {e}"))?;
            builder.with_client_cert_verifier(verifier)
        }
    };
    let config = builder
        .with_single_cert(chain, key)
        .map_err(|e| format!("configuration error: the TLS certificate and private key do not go together: {e}"))?;
    Ok(Prepared {
        config: Arc::new(config),
        warnings,
    })
}

/// What a finished handshake negotiated, for the admin console.
pub fn negotiated(conn: &rustls::ServerConnection) -> Negotiated {
    let version = match conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_3) => "TLS 1.3".to_string(),
        Some(rustls::ProtocolVersion::TLSv1_2) => "TLS 1.2".to_string(),
        Some(other) => format!("{other:?}"),
        None => String::new(),
    };
    let cipher = conn
        .negotiated_cipher_suite()
        .map(|s| format!("{:?}", s.suite()))
        .unwrap_or_default();
    let client_cert = conn
        .peer_certificates()
        .and_then(|chain| chain.first())
        .and_then(|leaf| subject(leaf.as_ref()));
    Negotiated {
        version,
        cipher,
        client_cert,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build, ConfigSource, FileConfig, Overrides};

    /// A self-signed certificate and its key, as PEM.
    fn self_signed(name: &str, days: i64) -> (String, String) {
        let mut params = rcgen::CertificateParams::new(vec![name.to_string()]).unwrap();
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::days(1);
        params.not_after = now + time::Duration::days(days);
        params.distinguished_name = rcgen::DistinguishedName::new();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, name.to_string());
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mqrust-tls-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A configuration with TLS on, built as if `mqrust.toml` were in `dir`.
    fn cfg_in(dir: &std::path::Path, f: impl FnOnce(&mut FileConfig)) -> Result<Config, String> {
        let mut fc = FileConfig::default();
        fc.broker.allow_anonymous = true;
        fc.admin.password = Some("admin".into());
        fc.tls.enabled = true;
        f(&mut fc);
        build(fc, ConfigSource::File(dir.join("mqrust.toml")), &Overrides::default()).map_err(|e| e.0)
    }

    #[test]
    fn a_pem_pair_is_accepted() {
        let d = dir("pem");
        let (cert, key) = self_signed("broker.example", 365);
        std::fs::write(d.join("broker.crt"), &cert).unwrap();
        std::fs::write(d.join("broker.key"), &key).unwrap();
        let cfg = cfg_in(&d, |fc| {
            fc.tls.cert = "broker.crt".into();
            fc.tls.key = "broker.key".into();
        })
        .unwrap();
        let prepared = prepare(&cfg).unwrap();
        assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
    }

    #[test]
    fn a_key_of_another_certificate_is_refused() {
        let d = dir("mismatch");
        let (cert, _) = self_signed("broker.example", 365);
        let (_, other_key) = self_signed("other.example", 365);
        std::fs::write(d.join("broker.crt"), &cert).unwrap();
        std::fs::write(d.join("other.key"), &other_key).unwrap();
        let cfg = cfg_in(&d, |fc| {
            fc.tls.cert = "broker.crt".into();
            fc.tls.key = "other.key".into();
        })
        .unwrap();
        let e = prepare(&cfg).unwrap_err();
        assert!(e.contains("do not go together"), "{e}");
    }

    #[test]
    fn an_expiring_certificate_only_warns() {
        let d = dir("expiring");
        let (cert, key) = self_signed("broker.example", 10);
        std::fs::write(d.join("broker.crt"), &cert).unwrap();
        std::fs::write(d.join("broker.key"), &key).unwrap();
        let cfg = cfg_in(&d, |fc| {
            fc.tls.cert = "broker.crt".into();
            fc.tls.key = "broker.key".into();
        })
        .unwrap();
        let prepared = prepare(&cfg).unwrap();
        assert_eq!(prepared.warnings.len(), 1, "{:?}", prepared.warnings);
        assert!(prepared.warnings[0].contains("expires on"), "{:?}", prepared.warnings);
        assert!(prepared.warnings[0].contains("10 day(s)"), "{:?}", prepared.warnings);
    }

    #[test]
    fn a_missing_file_names_the_key() {
        let d = dir("missing");
        let cfg = cfg_in(&d, |fc| {
            fc.tls.cert = "nope.crt".into();
            fc.tls.key = "nope.key".into();
        })
        .unwrap();
        let e = prepare(&cfg).unwrap_err();
        assert!(e.contains("tls.cert") && e.contains("nope.crt"), "{e}");
    }

    #[test]
    fn a_jks_keystore_suggests_the_conversion() {
        let d = dir("jks");
        std::fs::write(d.join("broker.jks"), [0xfe, 0xed, 0xfe, 0xed, 0, 0, 0, 2]).unwrap();
        let cfg = cfg_in(&d, |fc| fc.tls.keystore = "broker.jks".into()).unwrap();
        let e = prepare(&cfg).unwrap_err();
        assert!(e.contains("tls.keystore"), "{e}");
        assert!(e.contains("JKS") && e.contains("keytool -importkeystore"), "{e}");
    }

    #[test]
    fn a_text_file_is_not_a_certificate() {
        let d = dir("garbage");
        std::fs::write(d.join("broker.crt"), "not a certificate\n").unwrap();
        std::fs::write(d.join("broker.key"), "neither is this\n").unwrap();
        let cfg = cfg_in(&d, |fc| {
            fc.tls.cert = "broker.crt".into();
            fc.tls.key = "broker.key".into();
        })
        .unwrap();
        let e = prepare(&cfg).unwrap_err();
        assert!(e.contains("tls.cert") && e.contains("no certificate"), "{e}");
    }

    #[test]
    fn client_certificates_need_their_ca() {
        let d = dir("clientca");
        let (cert, key) = self_signed("broker.example", 365);
        std::fs::write(d.join("broker.crt"), &cert).unwrap();
        std::fs::write(d.join("broker.key"), &key).unwrap();
        // The CA bundle is checked and used.
        let (ca, _) = self_signed("ca.example", 365);
        std::fs::write(d.join("ca.crt"), &ca).unwrap();
        let cfg = cfg_in(&d, |fc| {
            fc.tls.cert = "broker.crt".into();
            fc.tls.key = "broker.key".into();
            fc.tls.client_auth = "required".into();
            fc.tls.client_ca = "ca.crt".into();
        })
        .unwrap();
        assert!(prepare(&cfg).is_ok());

        // Without the bundle the configuration itself is refused.
        let e = cfg_in(&d, |fc| {
            fc.tls.cert = "broker.crt".into();
            fc.tls.key = "broker.key".into();
            fc.tls.client_auth = "required".into();
        })
        .unwrap_err();
        assert!(e.contains("tls.client_ca"), "{e}");
    }

    #[test]
    fn the_subject_is_read_from_the_certificate() {
        let (cert, _) = self_signed("client.example", 365);
        let der = CertificateDer::from_pem_slice(cert.as_bytes()).unwrap();
        let subject = subject(der.as_ref()).unwrap();
        assert!(subject.contains("client.example"), "{subject}");
    }
}
