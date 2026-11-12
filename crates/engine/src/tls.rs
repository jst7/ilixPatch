//! Certificates: loading CA bundles and client identities (PEM, encrypted
//! PKCS#8 PEM, PKCS#12/PFX) and describing peer certificates.

use std::fs;
use std::net::IpAddr;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A peer certificate, as shown to the user (`CertificateInfo` in api.ts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateInfo {
    pub subject: String,
    pub issuer: String,
    /// ISO 8601 (UTC).
    pub not_before: String,
    pub not_after: String,
    /// SHA-256 fingerprint of the DER, upper-case hex with colons.
    pub sha256: String,
    /// Subject alternative names (DNS names and IP addresses).
    pub san: Vec<String>,
}

/// Parse a DER certificate. `None` if it is not valid X.509.
pub fn certificate_info(der: &[u8]) -> Option<CertificateInfo> {
    use x509_parser::extensions::GeneralName;
    let (_, cert) = x509_parser::parse_x509_certificate(der).ok()?;
    let san = match cert.subject_alternative_name() {
        Ok(Some(ext)) => ext
            .value
            .general_names
            .iter()
            .filter_map(|n| match n {
                GeneralName::DNSName(d) => Some(d.to_string()),
                GeneralName::IPAddress(b) => match b.len() {
                    4 => Some(IpAddr::from(<[u8; 4]>::try_from(*b).ok()?).to_string()),
                    16 => Some(IpAddr::from(<[u8; 16]>::try_from(*b).ok()?).to_string()),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    let validity = cert.validity();
    Some(CertificateInfo {
        subject: cert.subject().to_string(),
        issuer: cert.issuer().to_string(),
        not_before: iso8601(validity.not_before.timestamp()),
        not_after: iso8601(validity.not_after.timestamp()),
        sha256: fingerprint(der),
        san,
    })
}

/// SHA-256 fingerprint, `AB:CD:…`.
pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Unix timestamp → `YYYY-MM-DDTHH:MM:SSZ`.
pub fn iso8601(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// One `-----BEGIN <label>-----` block.
struct PemBlock {
    label: String,
    der: Vec<u8>,
}

fn pem_blocks(text: &str) -> Result<Vec<PemBlock>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN ") {
        let after = &rest[start + 11..];
        let label_end = after.find("-----").ok_or("malformed PEM header")?;
        let label = after[..label_end].to_string();
        let body_start = &after[label_end + 5..];
        let end_marker = format!("-----END {label}-----");
        let end = body_start
            .find(&end_marker)
            .ok_or_else(|| format!("missing '{end_marker}'"))?;
        let body: String = body_start[..end]
            .lines()
            .filter(|l| !l.contains(':')) // skip legacy "Proc-Type:" headers
            .flat_map(|l| l.chars())
            .filter(|c| !c.is_whitespace())
            .collect();
        let der = B64
            .decode(body.as_bytes())
            .map_err(|e| format!("invalid base64 in PEM '{label}' block: {e}"))?;
        out.push(PemBlock { label, der });
        rest = &body_start[end + end_marker.len()..];
    }
    Ok(out)
}

fn pem_encode(label: &str, der: &[u8]) -> String {
    let b = B64.encode(der);
    let mut s = format!("-----BEGIN {label}-----\n");
    for chunk in b.as_bytes().chunks(64) {
        s.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        s.push('\n');
    }
    s.push_str(&format!("-----END {label}-----\n"));
    s
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("{path}: {e}"))
}

fn read_text(path: &str) -> Result<String, String> {
    String::from_utf8(read(path)?).map_err(|_| format!("{path}: not a PEM text file"))
}

/// Load every certificate of a PEM bundle file.
pub fn load_ca_file(path: &str) -> Result<Vec<reqwest::Certificate>, String> {
    let text = read_text(path)?;
    let blocks = pem_blocks(&text).map_err(|e| format!("{path}: {e}"))?;
    let certs: Vec<_> = blocks
        .iter()
        .filter(|b| b.label == "CERTIFICATE" || b.label == "TRUSTED CERTIFICATE")
        .map(|b| {
            x509_parser::parse_x509_certificate(&b.der)
                .map_err(|e| format!("{path}: invalid certificate: {e}"))?;
            reqwest::Certificate::from_der(&b.der).map_err(|e| format!("{path}: {e}"))
        })
        .collect::<Result<_, _>>()?;
    if certs.is_empty() {
        return Err(format!("{path}: no PEM certificate found"));
    }
    Ok(certs)
}

/// Build the identity PEM (key + chain) from a certificate PEM file and a
/// key PEM file. Encrypted PKCS#8 keys (`ENCRYPTED PRIVATE KEY`) are
/// decrypted with `passphrase`.
pub fn identity_pem_from_files(
    cert_path: &str,
    key_path: Option<&str>,
    passphrase: Option<&str>,
) -> Result<String, String> {
    let cert_text = read_text(cert_path)?;
    let certs: Vec<PemBlock> = pem_blocks(&cert_text)
        .map_err(|e| format!("{cert_path}: {e}"))?
        .into_iter()
        .filter(|b| b.label == "CERTIFICATE")
        .collect();
    if certs.is_empty() {
        return Err(format!("{cert_path}: no PEM certificate found"));
    }
    // The key may live in the certificate file.
    let (key_text, key_src) = match key_path {
        Some(k) => (read_text(k)?, k),
        None => (cert_text.clone(), cert_path),
    };
    let key = pem_blocks(&key_text)
        .map_err(|e| format!("{key_src}: {e}"))?
        .into_iter()
        .find(|b| b.label.ends_with("PRIVATE KEY"))
        .ok_or_else(|| format!("{key_src}: no PEM private key found"))?;
    let key_pem = match key.label.as_str() {
        "ENCRYPTED PRIVATE KEY" => {
            let pass = passphrase.ok_or_else(|| {
                format!("{key_src}: the key is encrypted; a passphrase is required")
            })?;
            let info = pkcs8::EncryptedPrivateKeyInfoRef::try_from(key.der.as_slice())
                .map_err(|e| format!("{key_src}: {e}"))?;
            let doc = info.decrypt(pass).map_err(|_| {
                format!("{key_src}: wrong passphrase or unsupported key encryption")
            })?;
            pem_encode("PRIVATE KEY", doc.as_bytes())
        }
        "PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY" => {
            if key_text.contains("Proc-Type: 4,ENCRYPTED") {
                return Err(format!(
                    "{key_src}: legacy encrypted PEM keys are not supported; convert with \
                     `openssl pkcs8 -topk8` or use a PKCS#12 file"
                ));
            }
            pem_encode(&key.label, &key.der)
        }
        other => return Err(format!("{key_src}: unsupported key type '{other}'")),
    };
    let mut pem = key_pem;
    for c in &certs {
        pem.push_str(&pem_encode("CERTIFICATE", &c.der));
    }
    Ok(pem)
}

/// Convert a PKCS#12/PFX bundle into an identity PEM (key + chain).
pub fn identity_pem_from_pkcs12(path: &str, passphrase: Option<&str>) -> Result<String, String> {
    let data = read(path)?;
    let store = p12_keystore::KeyStore::from_pkcs12(
        &data,
        passphrase.unwrap_or_default(),
        p12_keystore::Pkcs12ImportPolicy::Relaxed,
    )
    .map_err(|e| format!("{path}: cannot read PKCS#12 (wrong passphrase?): {e}"))?;
    let (_, chain) = store
        .private_key_chain()
        .ok_or_else(|| format!("{path}: no private key in the PKCS#12 bundle"))?;
    if chain.certs().is_empty() {
        return Err(format!("{path}: no certificate for the private key"));
    }
    let mut pem = pem_encode("PRIVATE KEY", chain.key().as_der());
    for c in chain.certs() {
        pem.push_str(&pem_encode("CERTIFICATE", c.as_der()));
    }
    Ok(pem)
}

pub fn identity_from_pem(pem: &str, what: &str) -> Result<reqwest::Identity, String> {
    reqwest::Identity::from_pem(pem.as_bytes()).map_err(|e| format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601(1_767_225_599), "2025-12-31T23:59:59Z");
        assert_eq!(iso8601(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn pem_roundtrip() {
        let pem = pem_encode("CERTIFICATE", &[1, 2, 3, 250]);
        let blocks = pem_blocks(&pem).unwrap();
        assert_eq!(blocks[0].label, "CERTIFICATE");
        assert_eq!(blocks[0].der, vec![1, 2, 3, 250]);
        assert!(pem_blocks("-----BEGIN X-----\nAAAA\n").is_err());
    }
}
