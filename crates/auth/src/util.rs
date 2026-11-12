//! Shared helpers: encodings, hashes/HMACs, UTC formatting, randomness, URLs and PEM keys.

use crate::{AuthEnv, AuthError, HttpParts};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

pub(crate) fn invalid(field: &str, message: impl Into<String>) -> AuthError {
    AuthError::Invalid {
        field: field.into(),
        message: message.into(),
    }
}

// ---------------------------------------------------------------- encodings

pub(crate) fn b64(data: &[u8]) -> String {
    STANDARD.encode(data)
}

pub(crate) fn b64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Lenient base64 decoding: standard or URL-safe alphabet, with or without padding.
pub(crate) fn b64_decode_lenient(s: &str) -> Option<Vec<u8>> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '=')
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(cleaned)
        .ok()
}

pub(crate) fn hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// RFC 3986 unreserved characters stay; everything else is `%XX` (uppercase).
const RFC3986: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub(crate) fn pct(s: &str) -> String {
    utf8_percent_encode(s, RFC3986).to_string()
}

pub(crate) fn pct_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// `application/x-www-form-urlencoded` parsing (`+` = space).
pub(crate) fn parse_form(s: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(s.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Encode pairs as `a=b&c=d` with RFC 3986 percent-encoding (spaces as `%20`).
pub(crate) fn encode_pairs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub(crate) fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

/// Quote a value for an HTTP auth parameter (`"…"` with `\` escapes).
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------- hashing

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HashAlg {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

pub(crate) fn hash(alg: HashAlg, data: &[u8]) -> Vec<u8> {
    match alg {
        HashAlg::Sha1 => Sha1::digest(data).to_vec(),
        HashAlg::Sha256 => Sha256::digest(data).to_vec(),
        HashAlg::Sha384 => Sha384::digest(data).to_vec(),
        HashAlg::Sha512 => Sha512::digest(data).to_vec(),
    }
}

pub(crate) fn hmac(alg: HashAlg, key: &[u8], data: &[u8]) -> Vec<u8> {
    fn run<M: Mac + hmac::digest::KeyInit>(key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut m = <M as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
        m.update(data);
        m.finalize().into_bytes().to_vec()
    }
    match alg {
        HashAlg::Sha1 => run::<Hmac<Sha1>>(key, data),
        HashAlg::Sha256 => run::<Hmac<Sha256>>(key, data),
        HashAlg::Sha384 => run::<Hmac<Sha384>>(key, data),
        HashAlg::Sha512 => run::<Hmac<Sha512>>(key, data),
    }
}

pub(crate) fn hmac_md5(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = <Hmac<md5::Md5> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

// ---------------------------------------------------------------- time

/// (year, month, day, hour, minute, second) in UTC for Unix seconds.
pub(crate) fn utc(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

// ---------------------------------------------------------------- randomness

pub(crate) fn random_bytes(env: &mut AuthEnv, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    (env.random)(&mut buf);
    buf
}

pub(crate) fn random_alnum(env: &mut AuthEnv, n: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    random_bytes(env, n)
        .iter()
        .map(|b| CHARS[(*b as usize) % CHARS.len()] as char)
        .collect()
}

pub(crate) fn uuid_v4(env: &mut AuthEnv) -> String {
    let mut b = random_bytes(env, 16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// `rand_core` adapter over `AuthEnv.random` (RSA blinding, PSS salts).
pub(crate) struct EnvRng<'e, 'a>(pub &'e mut AuthEnv<'a>);

impl rand_core::RngCore for EnvRng<'_, '_> {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        (self.0.random)(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        (self.0.random)(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        (self.0.random)(dest)
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        (self.0.random)(dest);
        Ok(())
    }
}

impl rand_core::CryptoRng for EnvRng<'_, '_> {}

// ---------------------------------------------------------------- requests

pub(crate) fn parse_url(url: &str) -> Result<url::Url, AuthError> {
    url::Url::parse(url.trim())
        .map_err(|e| invalid("url", format!("not a valid absolute URL ({e}): {url}")))
}

/// Path and query as sent on the request line (`/a/b?x=1`).
pub(crate) fn request_target(u: &url::Url) -> String {
    match u.query() {
        Some(q) => format!("{}?{}", u.path(), q),
        None => u.path().to_string(),
    }
}

/// `host[:port]` as in the `Host` header (default ports omitted).
pub(crate) fn host_header(u: &url::Url) -> String {
    let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
    match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    }
}

pub(crate) fn header<'a>(req: &'a HttpParts, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Media type without parameters, lowercased (`application/json`).
pub(crate) fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

pub(crate) fn is_form(req: &HttpParts) -> bool {
    header(req, "content-type")
        .is_some_and(|ct| media_type(ct) == "application/x-www-form-urlencoded")
}

pub(crate) fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------- PEM keys

pub(crate) enum PrivateKey {
    Rsa(Box<rsa::RsaPrivateKey>),
    P256(p256::SecretKey),
    P384(p384::SecretKey),
    P521(p521::SecretKey),
}

/// Parse a PEM private key: PKCS#1 (`RSA PRIVATE KEY`), SEC1 (`EC PRIVATE KEY`) or PKCS#8
/// (`PRIVATE KEY`, RSA or EC P-256/P-384/P-521). Literal `\n` sequences (keys pasted from JSON)
/// are accepted. Encrypted keys are rejected.
pub(crate) fn parse_private_key(pem: &str, field: &str) -> Result<PrivateKey, AuthError> {
    use p256::pkcs8::DecodePrivateKey;
    use rsa::pkcs1::DecodeRsaPrivateKey;

    let text = pem.replace("\\n", "\n").replace('\r', "");
    if text.trim().is_empty() {
        return Err(invalid(field, "a PEM private key is required"));
    }
    let (label, block) = find_private_key_block(&text).ok_or_else(|| {
        invalid(field, "expected a PEM private key (-----BEGIN PRIVATE KEY----- / RSA PRIVATE KEY / EC PRIVATE KEY)")
    })?;
    if label.contains("ENCRYPTED") || block.contains("Proc-Type: 4,ENCRYPTED") {
        return Err(invalid(
            field,
            "encrypted private keys are not supported; export the key without a passphrase",
        ));
    }
    let bad = |what: &str| {
        invalid(
            field,
            format!("could not read the {what} private key: invalid PEM/DER data"),
        )
    };
    match label.as_str() {
        "RSA PRIVATE KEY" => rsa::RsaPrivateKey::from_pkcs1_pem(&block)
            .map(|k| PrivateKey::Rsa(Box::new(k)))
            .map_err(|_| bad("RSA (PKCS#1)")),
        "EC PRIVATE KEY" => {
            if let Ok(k) = p256::SecretKey::from_sec1_pem(&block) {
                return Ok(PrivateKey::P256(k));
            }
            if let Ok(k) = p384::SecretKey::from_sec1_pem(&block) {
                return Ok(PrivateKey::P384(k));
            }
            if let Ok(k) = p521::SecretKey::from_sec1_pem(&block) {
                return Ok(PrivateKey::P521(k));
            }
            Err(bad("EC (SEC1, P-256/P-384/P-521)"))
        }
        "PRIVATE KEY" => {
            if let Ok(k) = rsa::RsaPrivateKey::from_pkcs8_pem(&block) {
                return Ok(PrivateKey::Rsa(Box::new(k)));
            }
            if let Ok(k) = p256::SecretKey::from_pkcs8_pem(&block) {
                return Ok(PrivateKey::P256(k));
            }
            if let Ok(k) = p384::SecretKey::from_pkcs8_pem(&block) {
                return Ok(PrivateKey::P384(k));
            }
            if let Ok(k) = p521::SecretKey::from_pkcs8_pem(&block) {
                return Ok(PrivateKey::P521(k));
            }
            Err(bad("PKCS#8 (RSA or EC P-256/P-384/P-521)"))
        }
        other => Err(invalid(field, format!("unsupported PEM block \"{other}\""))),
    }
}

/// First `-----BEGIN …PRIVATE KEY-----` block (skips e.g. `EC PARAMETERS`), re-assembled.
fn find_private_key_block(text: &str) -> Option<(String, String)> {
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN ") {
        let after = &rest[start + 11..];
        let end_label = after.find("-----")?;
        let label = after[..end_label].trim().to_string();
        let end_marker = format!("-----END {label}-----");
        let end = rest[start..].find(&end_marker)?;
        let block = &rest[start..start + end + end_marker.len()];
        if label.ends_with("PRIVATE KEY") {
            // Normalise: trim every line (keys pasted with indentation).
            let lines: Vec<&str> = block
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            return Some((label, lines.join("\n") + "\n"));
        }
        rest = &rest[start + end + end_marker.len()..];
    }
    None
}

/// RSASSA-PKCS1-v1_5 with RSA blinding (randomness from `AuthEnv.random`). The signature itself
/// is deterministic; blinding only protects the private-key operation against timing attacks.
pub(crate) fn rsa_pkcs1v15_sign(
    key: &rsa::RsaPrivateKey,
    alg: HashAlg,
    msg: &[u8],
    env: &mut AuthEnv,
) -> Vec<u8> {
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{RandomizedSigner, SignatureEncoding};
    let mut rng = EnvRng(env);
    let k = key.clone();
    match alg {
        HashAlg::Sha1 => SigningKey::<Sha1>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
        HashAlg::Sha256 => SigningKey::<Sha256>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
        HashAlg::Sha384 => SigningKey::<Sha384>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
        HashAlg::Sha512 => SigningKey::<Sha512>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
    }
}

/// RSASSA-PSS with blinding; `AuthEnv.random` provides the salt (salt length = hash length)
/// and the blinding factor.
pub(crate) fn rsa_pss_sign(
    key: &rsa::RsaPrivateKey,
    alg: HashAlg,
    msg: &[u8],
    env: &mut AuthEnv,
) -> Vec<u8> {
    use rsa::pss::BlindedSigningKey;
    use rsa::signature::{RandomizedSigner, SignatureEncoding};
    let mut rng = EnvRng(env);
    let k = key.clone();
    match alg {
        HashAlg::Sha1 => BlindedSigningKey::<Sha1>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
        HashAlg::Sha256 => BlindedSigningKey::<Sha256>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
        HashAlg::Sha384 => BlindedSigningKey::<Sha384>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
        HashAlg::Sha512 => BlindedSigningKey::<Sha512>::new(k)
            .sign_with_rng(&mut rng, msg)
            .to_vec(),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Deterministic "randomness": 0, 1, 2, … (wrapping).
    pub fn counter() -> impl FnMut(&mut [u8]) {
        let mut n: u8 = 0;
        move |buf: &mut [u8]| {
            for b in buf {
                *b = n;
                n = n.wrapping_add(1);
            }
        }
    }

    /// Deterministic but well-distributed stream: SHA-256(seed || block index) blocks.
    /// Needed wherever RSA blinding rejection-samples random numbers (a counter never terminates).
    pub fn stream() -> impl FnMut(&mut [u8]) {
        let mut block: u64 = 0;
        let mut pool: Vec<u8> = Vec::new();
        move |buf: &mut [u8]| {
            for b in buf {
                if pool.is_empty() {
                    let mut input = b"ilixpatch-auth test rng".to_vec();
                    input.extend_from_slice(&block.to_le_bytes());
                    block += 1;
                    pool = Sha256::digest(&input).to_vec();
                    pool.reverse();
                }
                *b = pool.pop().expect("refilled");
            }
        }
    }

    pub fn parts<'a>(
        method: &'a str,
        url: &'a str,
        headers: &'a [(String, String)],
        body: &'a [u8],
    ) -> HttpParts<'a> {
        HttpParts {
            method,
            url,
            headers,
            body,
        }
    }

    pub fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// 2048-bit RSA test key (PKCS#8), generated for these tests only.
    pub const RSA_PKCS8: &str = include_str!("testdata/rsa_pkcs8.pem");
    pub const RSA_PKCS1: &str = include_str!("testdata/rsa_pkcs1.pem");
    pub const EC_P256_SEC1: &str = include_str!("testdata/ec_p256_sec1.pem");
    pub const EC_P256_PKCS8: &str = include_str!("testdata/ec_p256_pkcs8.pem");
    pub const EC_P384_PKCS8: &str = include_str!("testdata/ec_p384_pkcs8.pem");
    pub const EC_P521_SEC1: &str = include_str!("testdata/ec_p521_sec1.pem");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_conversion() {
        // 2015-08-30T12:36:00Z (AWS SigV4 test suite date).
        assert_eq!(utc(1_440_938_160), (2015, 8, 30, 12, 36, 0));
        assert_eq!(utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc(951_782_400), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn percent_encoding_rfc3986() {
        assert_eq!(pct("a b+c~d*é"), "a%20b%2Bc~d%2A%C3%A9");
    }

    #[test]
    fn pem_errors() {
        assert!(
            matches!(parse_private_key("", "private_key"), Err(AuthError::Invalid { field, .. }) if field == "private_key")
        );
        assert!(parse_private_key("not a key", "private_key").is_err());
        let bad = "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----";
        assert!(parse_private_key(bad, "private_key").is_err());
        let enc =
            "-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----";
        let e = parse_private_key(enc, "private_key")
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("encrypted"), "{e}");
    }

    #[test]
    fn pem_formats() {
        use test_support::*;
        assert!(matches!(
            parse_private_key(RSA_PKCS8, "k"),
            Ok(PrivateKey::Rsa(_))
        ));
        assert!(matches!(
            parse_private_key(RSA_PKCS1, "k"),
            Ok(PrivateKey::Rsa(_))
        ));
        assert!(matches!(
            parse_private_key(EC_P256_SEC1, "k"),
            Ok(PrivateKey::P256(_))
        ));
        assert!(matches!(
            parse_private_key(EC_P256_PKCS8, "k"),
            Ok(PrivateKey::P256(_))
        ));
        assert!(matches!(
            parse_private_key(EC_P384_PKCS8, "k"),
            Ok(PrivateKey::P384(_))
        ));
        assert!(matches!(
            parse_private_key(EC_P521_SEC1, "k"),
            Ok(PrivateKey::P521(_))
        ));
        // Escaped newlines, as pasted from a JSON credentials file.
        let escaped = EC_P256_PKCS8.replace('\n', "\\n");
        assert!(matches!(
            parse_private_key(&escaped, "k"),
            Ok(PrivateKey::P256(_))
        ));
    }
}
