//! NTLMv2 over HTTP (MS-NLMP): Type 1 negotiate, Type 2 challenge parsing, Type 3 authenticate.
//!
//! No MIC and no session key exchange: HTTP authentication only needs the NTLMv2 response.

use crate::util::{self, b64, hmac_md5, invalid, utf16le};
use crate::{AuthEnv, AuthError};
use base64::Engine as _;
use ilixpatch_core::auth::NtlmAuth;
use md4::{Digest, Md4};

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

const NEGOTIATE_UNICODE: u32 = 0x0000_0001;
const NEGOTIATE_OEM: u32 = 0x0000_0002;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NEGOTIATE_NTLM: u32 = 0x0000_0200;
const NEGOTIATE_ALWAYS_SIGN: u32 = 0x0000_8000;
const NEGOTIATE_EXTENDED_SESSIONSECURITY: u32 = 0x0008_0000;
const NEGOTIATE_TARGET_INFO: u32 = 0x0080_0000;
const NEGOTIATE_128: u32 = 0x2000_0000;
const NEGOTIATE_56: u32 = 0x8000_0000;

const CLIENT_FLAGS: u32 = NEGOTIATE_UNICODE
    | NEGOTIATE_OEM
    | REQUEST_TARGET
    | NEGOTIATE_NTLM
    | NEGOTIATE_ALWAYS_SIGN
    | NEGOTIATE_EXTENDED_SESSIONSECURITY
    | NEGOTIATE_TARGET_INFO
    | NEGOTIATE_128
    | NEGOTIATE_56;

const AV_EOL: u16 = 0;
const AV_TIMESTAMP: u16 = 7;

/// Seconds between 1601-01-01 and 1970-01-01.
const FILETIME_EPOCH_OFFSET: u64 = 11_644_473_600;

/// `(user, domain)`: an explicit domain wins; otherwise `DOMAIN\user` or `user@domain`.
fn split_user(cfg: &NtlmAuth) -> (String, String) {
    let domain = cfg.domain.as_deref().map(str::trim).unwrap_or_default();
    let user = cfg.username.trim();
    if !domain.is_empty() {
        return (user.to_string(), domain.to_string());
    }
    if let Some((d, u)) = user.split_once('\\') {
        return (u.to_string(), d.to_string());
    }
    if let Some((u, d)) = user.split_once('@') {
        return (u.to_string(), d.to_string());
    }
    (user.to_string(), String::new())
}

/// `Authorization: NTLM <base64 Type 1>` for the first request.
pub fn negotiate(cfg: &NtlmAuth) -> Result<String, AuthError> {
    if cfg.username.trim().is_empty() {
        return Err(invalid("username", "is required for NTLM"));
    }
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&1u32.to_le_bytes());
    m.extend_from_slice(&CLIENT_FLAGS.to_le_bytes());
    // Empty DomainName and Workstation fields (offset = end of message).
    for _ in 0..2 {
        m.extend_from_slice(&0u16.to_le_bytes());
        m.extend_from_slice(&0u16.to_le_bytes());
        m.extend_from_slice(&32u32.to_le_bytes());
    }
    Ok(format!("NTLM {}", b64(&m)))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Type2 {
    pub flags: u32,
    pub server_challenge: [u8; 8],
    pub target_info: Vec<u8>,
    /// MsvAvTimestamp (FILETIME), when the server sent one.
    pub timestamp: Option<u64>,
}

fn read_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

pub(crate) fn parse_type2(msg: &[u8]) -> Result<Type2, AuthError> {
    let bad = |m: &str| invalid("challenge", format!("invalid NTLM challenge message: {m}"));
    if msg.len() < 32 || &msg[..8] != SIGNATURE {
        return Err(bad("missing NTLMSSP signature"));
    }
    if read_u32(msg, 8) != Some(2) {
        return Err(bad("not a Type 2 (CHALLENGE) message"));
    }
    let flags = read_u32(msg, 20).ok_or_else(|| bad("truncated"))?;
    let mut server_challenge = [0u8; 8];
    server_challenge.copy_from_slice(&msg[24..32]);
    let mut target_info = Vec::new();
    let mut timestamp = None;
    if msg.len() >= 48 {
        let len = read_u16(msg, 40).unwrap_or(0) as usize;
        let off = read_u32(msg, 44).unwrap_or(0) as usize;
        if len > 0 {
            target_info = msg
                .get(off..off + len)
                .ok_or_else(|| bad("target info out of bounds"))?
                .to_vec();
            let mut p = 0;
            while p + 4 <= target_info.len() {
                let id = read_u16(&target_info, p).unwrap_or(0);
                let l = read_u16(&target_info, p + 2).unwrap_or(0) as usize;
                if id == AV_EOL {
                    break;
                }
                let v = target_info
                    .get(p + 4..p + 4 + l)
                    .ok_or_else(|| bad("AV pair out of bounds"))?;
                if id == AV_TIMESTAMP && l == 8 {
                    timestamp = Some(u64::from_le_bytes(v.try_into().expect("8 bytes")));
                }
                p += 4 + l;
            }
        }
    }
    Ok(Type2 {
        flags,
        server_challenge,
        target_info,
        timestamp,
    })
}

/// NTOWFv2 = HMAC-MD5(MD4(UTF-16LE(password)), UTF-16LE(UPPER(user) + domain)).
pub(crate) fn ntowf_v2(user: &str, domain: &str, password: &str) -> Vec<u8> {
    let nt_hash = Md4::digest(utf16le(password));
    hmac_md5(
        &nt_hash,
        &utf16le(&format!("{}{}", user.to_uppercase(), domain)),
    )
}

/// (LmChallengeResponse, NtChallengeResponse) per MS-NLMP §3.3.2.
pub(crate) fn responses(
    ntowf: &[u8],
    server_challenge: &[u8; 8],
    client_challenge: &[u8; 8],
    time: u64,
    target_info: &[u8],
) -> (Vec<u8>, Vec<u8>) {
    let mut temp = vec![1u8, 1, 0, 0, 0, 0, 0, 0];
    temp.extend_from_slice(&time.to_le_bytes());
    temp.extend_from_slice(client_challenge);
    temp.extend_from_slice(&[0; 4]);
    temp.extend_from_slice(target_info);
    temp.extend_from_slice(&[0; 4]);
    let mut buf = server_challenge.to_vec();
    buf.extend_from_slice(&temp);
    let mut nt = hmac_md5(ntowf, &buf);
    nt.extend_from_slice(&temp);
    let mut buf = server_challenge.to_vec();
    buf.extend_from_slice(client_challenge);
    let mut lm = hmac_md5(ntowf, &buf);
    lm.extend_from_slice(client_challenge);
    (lm, nt)
}

/// The base64 Type 2 message from `WWW-Authenticate: NTLM <…>` values.
fn find_challenge(www_authenticate: &[String]) -> Option<Vec<u8>> {
    for value in www_authenticate {
        for part in value.split(',') {
            let part = part.trim();
            if part.len() > 5 && part[..5].eq_ignore_ascii_case("ntlm ") {
                return util::b64_decode_lenient(part[5..].trim()).or_else(|| {
                    base64::engine::general_purpose::STANDARD
                        .decode(part[5..].trim())
                        .ok()
                });
            }
        }
    }
    None
}

/// Answer the server's `WWW-Authenticate: NTLM <base64 Type 2>` with a Type 3 (NTLMv2) header value.
pub fn authenticate(
    cfg: &NtlmAuth,
    www_authenticate: &[String],
    env: &mut AuthEnv,
) -> Result<String, AuthError> {
    if cfg.username.trim().is_empty() {
        return Err(invalid("username", "is required for NTLM"));
    }
    let msg = find_challenge(www_authenticate).ok_or_else(|| {
        invalid(
            "challenge",
            "the server did not send an NTLM challenge (WWW-Authenticate: NTLM <token>)",
        )
    })?;
    let t2 = parse_type2(&msg)?;
    let (user, domain) = split_user(cfg);
    let workstation = cfg
        .workstation
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let mut client_challenge = [0u8; 8];
    (env.random)(&mut client_challenge);
    let time = t2
        .timestamp
        .unwrap_or((env.now + FILETIME_EPOCH_OFFSET) * 10_000_000);
    let ntowf = ntowf_v2(&user, &domain, &cfg.password);
    let (mut lm, nt) = responses(
        &ntowf,
        &t2.server_challenge,
        &client_challenge,
        time,
        &t2.target_info,
    );
    if t2.timestamp.is_some() {
        // MS-NLMP §3.1.5.1.2: with MsvAvTimestamp the client sends Z(24) as the LM response.
        lm = vec![0; 24];
    }
    Ok(format!(
        "NTLM {}",
        b64(&type3(t2.flags, &lm, &nt, &domain, &user, &workstation))
    ))
}

fn type3(
    server_flags: u32,
    lm: &[u8],
    nt: &[u8],
    domain: &str,
    user: &str,
    workstation: &str,
) -> Vec<u8> {
    let unicode = server_flags & NEGOTIATE_UNICODE != 0;
    let enc = |s: &str| {
        if unicode {
            utf16le(s)
        } else {
            s.to_uppercase().into_bytes()
        }
    };
    let mut flags = (server_flags & CLIENT_FLAGS) | NEGOTIATE_NTLM;
    if unicode {
        flags &= !NEGOTIATE_OEM;
    }
    let fields: [Vec<u8>; 6] = [
        lm.to_vec(),
        nt.to_vec(),
        enc(domain),
        enc(user),
        enc(workstation),
        Vec::new(),
    ];
    let header_len = 64usize;
    let mut m = Vec::new();
    m.extend_from_slice(SIGNATURE);
    m.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = header_len;
    for f in &fields {
        m.extend_from_slice(&(f.len() as u16).to_le_bytes());
        m.extend_from_slice(&(f.len() as u16).to_le_bytes());
        m.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += f.len();
    }
    m.extend_from_slice(&flags.to_le_bytes());
    debug_assert_eq!(m.len(), header_len);
    for f in &fields {
        m.extend_from_slice(f);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // MS-NLMP §4.2.4 "NTLMv2 Authentication" example values.
    const CHALLENGE_MSG: &str = "4e 54 4c 4d 53 53 50 00 02 00 00 00 0c 00 0c 00 38 00 00 00 33 82 8a e2 01 23 45 67 89 ab cd ef \
        00 00 00 00 00 00 00 00 24 00 24 00 44 00 00 00 06 00 70 17 00 00 00 0f 53 00 65 00 72 00 76 00 65 00 72 00 \
        02 00 0c 00 44 00 6f 00 6d 00 61 00 69 00 6e 00 01 00 0c 00 53 00 65 00 72 00 76 00 65 00 72 00 00 00 00 00";

    #[test]
    fn ms_nlmp_4_2_4_ntlmv2() {
        let ntowf = ntowf_v2("User", "Domain", "Password");
        assert_eq!(
            ntowf,
            unhex("0c 86 8a 40 3b fd 7a 93 a3 00 1e f2 2e f0 2e 3f")
        );

        let t2 = parse_type2(&unhex(CHALLENGE_MSG)).unwrap();
        assert_eq!(
            t2.server_challenge,
            [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(t2.flags, 0xe28a_8233);
        assert_eq!(t2.timestamp, None);
        assert_eq!(t2.target_info.len(), 36);

        let (lm, nt) = responses(&ntowf, &t2.server_challenge, &[0xaa; 8], 0, &t2.target_info);
        assert_eq!(
            lm,
            unhex("86 c3 50 97 ac 9c ec 10 25 54 76 4a 57 cc cc 19 aa aa aa aa aa aa aa aa")
        );
        assert_eq!(
            &nt[..16],
            &unhex("68 cd 0a b8 51 e5 1c 96 aa bc 92 7b eb ef 6a 1c")[..]
        );
    }

    #[test]
    fn full_handshake() {
        let cfg = NtlmAuth {
            username: "Domain\\User".into(),
            password: "Password".into(),
            workstation: Some("COMPUTER".into()),
            ..Default::default()
        };
        let t1 = negotiate(&cfg).unwrap();
        let raw = util::b64_decode_lenient(t1.strip_prefix("NTLM ").unwrap()).unwrap();
        assert_eq!(&raw[..8], SIGNATURE);
        assert_eq!(read_u32(&raw, 8), Some(1));
        assert_eq!(raw.len(), 32);

        let www = vec![format!("Negotiate, NTLM {}", b64(&unhex(CHALLENGE_MSG)))];
        let mut rnd = |b: &mut [u8]| b.fill(0xaa);
        let mut env = AuthEnv {
            now: 1_700_000_000,
            random: &mut rnd,
        };
        let t3 = authenticate(&cfg, &www, &mut env).unwrap();
        let m = util::b64_decode_lenient(t3.strip_prefix("NTLM ").unwrap()).unwrap();
        assert_eq!(read_u32(&m, 8), Some(3));
        let field = |i: usize| {
            let at = 12 + i * 8;
            let len = read_u16(&m, at).unwrap() as usize;
            let off = read_u32(&m, at + 4).unwrap() as usize;
            m[off..off + len].to_vec()
        };
        // LMv2 does not depend on the time: matches the MS-NLMP example.
        assert_eq!(
            field(0),
            unhex("86 c3 50 97 ac 9c ec 10 25 54 76 4a 57 cc cc 19 aa aa aa aa aa aa aa aa")
        );
        // NTLMv2 response with the time derived from `now`.
        let time = (1_700_000_000 + FILETIME_EPOCH_OFFSET) * 10_000_000;
        let ntowf = ntowf_v2("User", "Domain", "Password");
        let t2 = parse_type2(&unhex(CHALLENGE_MSG)).unwrap();
        let (_, nt) = responses(
            &ntowf,
            &t2.server_challenge,
            &[0xaa; 8],
            time,
            &t2.target_info,
        );
        assert_eq!(field(1), nt);
        assert_eq!(field(2), utf16le("Domain"));
        assert_eq!(field(3), utf16le("User"));
        assert_eq!(field(4), utf16le("COMPUTER"));
        assert!(field(5).is_empty());
        let flags = read_u32(&m, 60).unwrap();
        assert!(flags & NEGOTIATE_UNICODE != 0 && flags & NEGOTIATE_NTLM != 0);
    }

    #[test]
    fn timestamp_av_pair() {
        // Challenge with MsvAvTimestamp: the time comes from the server, LM response is Z(24).
        let mut ti = Vec::new();
        ti.extend_from_slice(&AV_TIMESTAMP.to_le_bytes());
        ti.extend_from_slice(&8u16.to_le_bytes());
        ti.extend_from_slice(&0x01d0_0000_0000_0000u64.to_le_bytes());
        ti.extend_from_slice(&[0, 0, 0, 0]);
        let mut msg = Vec::new();
        msg.extend_from_slice(SIGNATURE);
        msg.extend_from_slice(&2u32.to_le_bytes());
        msg.extend_from_slice(&[0, 0, 0, 0, 48, 0, 0, 0]);
        msg.extend_from_slice(&(NEGOTIATE_UNICODE | NEGOTIATE_NTLM).to_le_bytes());
        msg.extend_from_slice(&[9; 8]);
        msg.extend_from_slice(&[0; 8]);
        msg.extend_from_slice(&(ti.len() as u16).to_le_bytes());
        msg.extend_from_slice(&(ti.len() as u16).to_le_bytes());
        msg.extend_from_slice(&48u32.to_le_bytes());
        msg.extend_from_slice(&ti);
        let t2 = parse_type2(&msg).unwrap();
        assert_eq!(t2.timestamp, Some(0x01d0_0000_0000_0000));
        let cfg = NtlmAuth {
            username: "u".into(),
            password: "p".into(),
            domain: Some("D".into()),
            ..Default::default()
        };
        let mut rnd = |b: &mut [u8]| b.fill(1);
        let mut env = AuthEnv {
            now: 0,
            random: &mut rnd,
        };
        let t3 = authenticate(&cfg, &[format!("NTLM {}", b64(&msg))], &mut env).unwrap();
        let m = util::b64_decode_lenient(&t3[5..]).unwrap();
        let off = read_u32(&m, 16).unwrap() as usize;
        assert_eq!(&m[off..off + 24], &[0u8; 24]);
    }

    #[test]
    fn errors() {
        let cfg = NtlmAuth {
            username: "u".into(),
            ..Default::default()
        };
        let mut rnd = |b: &mut [u8]| b.fill(1);
        let mut env = AuthEnv {
            now: 0,
            random: &mut rnd,
        };
        assert!(
            matches!(negotiate(&NtlmAuth::default()), Err(AuthError::Invalid { field, .. }) if field == "username")
        );
        assert!(
            matches!(authenticate(&cfg, &["Basic realm=\"x\"".into()], &mut env), Err(AuthError::Invalid { field, .. }) if field == "challenge")
        );
        assert!(authenticate(&cfg, &["NTLM AAAA".into()], &mut env).is_err());
    }
}
