# ilixpatch-engine

Sends fully-resolved requests with `reqwest` (rustls). Requests are sent from
Rust, never from the webview, so browser CORS rules do not apply (like Postman).

## Network settings (D29)

`Engine::with_settings(NetworkSettings)` loads and validates everything up
front. Errors name the field that is wrong, e.g.
`client_certificates[1].pfx_path: … wrong passphrase`. `Engine::new()` uses the
defaults: verify TLS, OS and bundled roots, system proxy, follow up to 10
redirects, no timeout, automatic HTTP version.

| Setting | Behaviour |
|---|---|
| `verify_ssl` | `false` accepts any certificate and host name (`danger_accept_invalid_certs`). |
| `use_system_ca` | Trust the OS root store (`rustls-native-certs`). The bundled Mozilla roots (webpki-roots) are always trusted. |
| `ca_certificates` | Extra PEM bundles (all `CERTIFICATE` blocks are added). |
| `client_certificates` | mTLS identities, chosen per request by host pattern (`api.example.com`, `*.example.com` = subdomains only, `*` = any, optional `:port`). The first enabled match wins. Accepted formats: PEM cert + PEM key (PKCS#8, RSA or SEC1; encrypted PKCS#8 with `passphrase`), or PKCS#12/PFX (converted to PEM in pure Rust with `p12-keystore`, including legacy RC2/3DES bundles). Legacy OpenSSL-encrypted PEM keys (`Proc-Type: 4,ENCRYPTED`) are rejected with a hint. |
| `proxy.mode` | `system`: reqwest's default (`HTTP(S)_PROXY`/`ALL_PROXY`/`NO_PROXY` plus the macOS/Windows system proxy). `none`: no proxy. `custom`: `url` (http, https, socks5, socks5h) with optional `username`/`password` and a `bypass` list in `NO_PROXY` syntax (`localhost`, `.internal`, `*.corp`, `10.0.0.0/8`, `host:port`, `*`). |
| `follow_redirects`, `max_redirects` | The engine follows redirects itself (reqwest's policy is `none`) so every hop is recorded. 301/302 turn POST into GET, 303 turns anything except HEAD into GET (the body is dropped). `Authorization`, `Cookie` and `Proxy-Authorization` are dropped on cross-origin hops. Going over the limit is an error. |
| `timeout_ms` | Total time for the request, redirects included (0 = none). |
| `min_tls_version` | `"1.2"` or `"1.3"`. |
| `http_version` | `auto` (ALPN), `http1` (`http1_only`), `http2` (prior knowledge on `http://`, ALPN on `https://`). |

### Per-request overrides

`Request.settings` (`[settings]` in the request TOML; `verify_ssl`,
`follow_redirects`, `max_redirects`, `timeout_ms`, `http_version`) overrides
the global values. For the timeout, the request's `timeout_ms` beats the
caller's timeout (`send_with_timeout`, e.g. `--timeout-request` or the runner's
`timeout_ms`), which beats the global `timeout_ms`.

### Client cache

One `reqwest::Client` is built per (verify, HTTP version mode, client identity)
combination and cached, so connection pooling and TLS session reuse still
work. Saving new settings in the app builds a new `Engine`. Requests already
running keep the old one.

### Response network info

`Response.network`: `http_version`, `remote_addr`, `redirects`
(`{status, url}` per hop), `final_url`, `tls` (peer certificate with subject,
issuer, validity in ISO 8601, SHA-256 fingerprint and SANs, parsed with
`x509-parser`; `verified` is the effective `verify_ssl`) and `proxy` (the proxy
URL without credentials). In `system` mode, `proxy` is reported from the
environment variables only.

## Persistence

`load_settings` / `save_settings` read and write `network.toml` in the app
config dir. The file is written with mode `0600` on Unix because it may hold
passphrases and proxy passwords. The CLI reads it only with
`--use-app-settings`.
