//! Per-request network overrides (D29), stored as `[settings]` in a request
//! file. `None` means "use the global network settings".

use serde::{Deserialize, Serialize};

/// HTTP version preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersionPref {
    /// HTTP/2 when the server offers it over TLS (ALPN), else HTTP/1.1.
    #[default]
    Auto,
    /// Force HTTP/1.1.
    Http1,
    /// HTTP/2 (prior knowledge on plain `http://`; ALPN on `https://`).
    Http2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RequestSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_ssl: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_redirects: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_redirects: Option<u32>,
    /// 0 = no timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_version: Option<HttpVersionPref>,
}

impl RequestSettings {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Request;

    #[test]
    fn settings_roundtrip_and_omitted_when_empty() {
        let req: Request = toml::from_str(
            "name = \"x\"\nurl = \"http://a\"\n[settings]\nverify_ssl = false\nhttp_version = \"http1\"\n",
        )
        .unwrap();
        assert_eq!(req.settings.verify_ssl, Some(false));
        assert_eq!(req.settings.http_version, Some(HttpVersionPref::Http1));
        let out = toml::to_string(&req).unwrap();
        assert!(out.contains("[settings]") && out.contains("verify_ssl = false"));

        let plain: Request = toml::from_str("name = \"x\"\nurl = \"http://a\"\n").unwrap();
        assert!(plain.settings.is_empty());
        assert!(!toml::to_string(&plain).unwrap().contains("settings"));
    }
}
