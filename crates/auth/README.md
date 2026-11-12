# ilixpatch-auth

Authorization signers for ilixPatch (D39). The functions are pure: they do no
I/O and use no global clock or RNG, because time and randomness come in through
`AuthEnv`. The model (`ilixpatch_core::auth::Auth`) is resolved for variables
before it gets here. Errors name the model field that is wrong
(`AuthError::Invalid { field, message }`).

## Entry points

| API | Use |
|---|---|
| `sign(auth, req, env)` | Single-shot types → `AuthOutput` (headers to set, query params to append, optional body). |
| `digest::{parse_challenge, authorization, preemptive}` | Digest challenge/response. `preemptive` works when `realm` and `nonce` are configured. |
| `ntlm::{negotiate, authenticate}` | NTLMv2 handshake (Type 1 → Type 2 → Type 3). The connection must stay the same for the whole exchange. |
| `oauth2::{pkce, authorize_url, token_request, parse_token_response, parse_redirect, apply_token}` | OAuth 2.0 flows, without network access. |
| `asap_token(cfg, env)` | Builds an ASAP JWT. |

`sign` returns `Unsupported` for `inherit` (the caller resolves it), for NTLM,
for Digest without `realm` and `nonce`, and for OAuth 2.0 without a literal
`access_token`.

## Supported types

| Type | Behaviour |
|---|---|
| No Auth / Inherit | Empty output / `Unsupported`. |
| API Key | Header or query param. As in Postman, nothing is sent while the key name is empty. |
| Bearer | `Authorization: Bearer <token>`. An empty token sends nothing. |
| Basic | `Authorization: Basic base64(user:pass)`, encoded as UTF-8. |
| JWT Bearer | HS256/384/512 (secret, optionally base64), RS*/PS* (RSA PEM in PKCS#1 or PKCS#8), ES256/384/512 (P-256/P-384/P-521 PEM in SEC1 or PKCS#8, RFC 6979 deterministic). The header always has `typ: JWT` and extra header fields are merged in. `alg` cannot be overridden. Output goes to a header with a prefix (default `Bearer`) or to a query param (default `token`). |
| Digest | MD5, SHA-256 and SHA-512-256, each also with `-sess`. qop `auth`, `auth-int` or none. Echoes `opaque`. The parser accepts several challenges per header, quoted or unquoted params and any case, and picks the strongest algorithm offered. |
| OAuth 1.0 | RFC 5849 signature base string. HMAC-SHA1/256/512, RSA-SHA1/256/512 (PKCS#1 v1.5) and PLAINTEXT. Supports realm, callback, verifier, `oauth_body_hash` (for non-form bodies) and empty params. Output goes to a header, the query or the form body. |
| Hawk | Hawk 1 header, sha256 or sha1, optional payload hash, `ext`, `app` and `dlg`. |
| AWS SigV4 | Header signing (`Authorization`, `X-Amz-Date`, plus `X-Amz-Content-Sha256` for s3 and `X-Amz-Security-Token`), or a presigned query (`add_to = query`, `X-Amz-Expires=86400`). |
| NTLM | NTLMv2 + LMv2 (Z(24) when the server sends a timestamp), Unicode/OEM, `DOMAIN\user` and `user@domain`. No MIC and no session key. |
| Akamai EdgeGrid | `EG1-HMAC-SHA256`, `headers_to_sign`, and a POST body hash capped at `max_body_size` (default 131072). |
| ASAP | JWT with `iss`, `sub` (default iss), `aud`, `iat`, `exp` (default +3600 s), `jti` (UUID v4) and `kid`. Uses `additional_claims`. |
| OAuth 2.0 | PKCE S256/plain. Authorize URL for code and implicit; the existing query is kept. Token requests for code/password/client_credentials/refresh, with client auth as a Basic header (not form-encoded, like Postman) or in the body (without a secret, `client_id` goes in the body). Responses can be JSON or form, and `error`/`error_description` become readable errors. The redirect parser handles query and fragment and checks `state`. |

## Dependencies

All pure Rust (RustCrypto): `sha1`, `sha2`, `md-5`, `md4`, `hmac`, `rsa`,
`p256`/`p384`/`p521`, `rand_core` (an adapter over `AuthEnv.random` that
supplies RSA blinding for PKCS#1 v1.5 and PSS plus PSS salts; callers pass an OS RNG.
ECDSA nonces are deterministic per RFC 6979), `base64`, `url` and `percent-encoding`.

## Test vectors

| Area | Source |
|---|---|
| Basic | RFC 7617 §2 and §2.1 |
| Digest | RFC 2617 §3.5, RFC 7616 §3.9.1 (SHA-256 and MD5) |
| NTLM | MS-NLMP §4.2.4 (NTOWFv2, LMv2, NTProofStr, challenge message) |
| OAuth 1.0 | RFC 5849 §3.4.1 base string; the Twitter "Creating a signature" example; the OAuth Body Hash example |
| Hawk | Hawk README header and payload examples |
| AWS SigV4 | aws-sig-v4-test-suite: get-vanilla, get-vanilla-query-order-key-case, post-x-www-form-urlencoded |
| Akamai | edgegrid-python `testdata.json` + all 13 signing cases of `testcases.json` |
| PKCE | RFC 7636 Appendix B |
| JWT | jwt.io HS256 example. RS/PS/ES signatures are checked against the derived public keys. |

The test keys in `src/testdata/` were generated for these tests only.
