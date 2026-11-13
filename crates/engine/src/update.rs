//! "Is there a newer version?" against the GitHub releases (D54).
//!
//! Only *published* releases count: GitHub's `releases/latest` never returns
//! drafts or pre-releases, so a release whose builds failed (left as a draft)
//! is never announced. Nothing is downloaded or installed: callers show the
//! release page.

use serde::{Deserialize, Serialize};

use crate::Engine;

/// `owner/repo` on GitHub.
pub const REPOSITORY: &str = "jst7/ilixPatch";

/// The GitHub API response is tiny; anything bigger is not it.
const MAX_RESPONSE: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestRelease {
    /// Without the leading `v`, e.g. `0.4.2`.
    pub version: String,
    /// The release page (installers, CLI binaries, notes).
    pub url: String,
    pub published_at: Option<String>,
}

/// `UpdateCheck` in api.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: LatestRelease,
    /// `latest` is newer than `current`.
    pub newer: bool,
}

/// `X.Y.Z` (optional `v`, a `-pre`/`+build` suffix is ignored).
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(v)
}

/// Whether `latest` is a higher version than `current` (false if either is unparsable).
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parse_version(current), parse_version(latest)), (Some(c), Some(l)) if l > c)
}

/// Read GitHub's `GET /repos/{repo}/releases/latest` response.
pub fn parse_latest(json: &[u8]) -> Result<LatestRelease, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        published_at: Option<String>,
    }
    let r: Release = serde_json::from_slice(json)
        .map_err(|e| format!("unexpected response from GitHub: {e}"))?;
    if parse_version(&r.tag_name).is_none() {
        return Err(format!(
            "the latest release tag \"{}\" is not a version",
            r.tag_name
        ));
    }
    Ok(LatestRelease {
        version: r.tag_name.trim_start_matches(['v', 'V']).to_string(),
        url: r.html_url,
        published_at: r.published_at,
    })
}

impl Engine {
    /// Compare `current` with the latest published release on GitHub,
    /// using the engine's network settings (proxy, CAs).
    pub async fn check_for_update(&self, current: &str) -> Result<UpdateCheck, String> {
        let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
        self.check_for_update_at(&url, current).await
    }

    pub(crate) async fn check_for_update_at(
        &self,
        url: &str,
        current: &str,
    ) -> Result<UpdateCheck, String> {
        let body = self.download(url, MAX_RESPONSE).await.map_err(|e| {
            // Unauthenticated API calls are limited per IP (60/hour).
            if e.starts_with("HTTP 403") || e.starts_with("HTTP 429") {
                format!("GitHub refused the request for now (rate limit?): {e}")
            } else if e.starts_with("HTTP 404") {
                "no published release found on GitHub".to_string()
            } else {
                e
            }
        })?;
        let latest = parse_latest(&body)?;
        Ok(UpdateCheck {
            current: current.to_string(),
            newer: is_newer(current, &latest.version),
            latest,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.4.1", "0.4.2"));
        assert!(is_newer("0.4.1", "v0.10.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("0.4.1", "0.4.1"));
        assert!(!is_newer("0.4.2", "0.4.1"));
        // Pre-release / build suffixes compare by their X.Y.Z.
        assert!(!is_newer("0.4.1", "0.4.1-rc.1"));
        assert!(is_newer("0.4.1-dev", "0.4.2+build.5"));
        assert!(!is_newer("0.4.1", "latest"));
        assert!(!is_newer("0.4", "0.5.0"));
        assert!(!is_newer("0.4.1", "0.4.2.1"));
    }

    #[test]
    fn github_response() {
        let r = parse_latest(
            br#"{"tag_name":"v0.4.2","name":"ilixPatch v0.4.2","draft":false,
                 "html_url":"https://github.com/jst7/ilixPatch/releases/tag/v0.4.2",
                 "published_at":"2026-11-13T10:00:00Z","assets":[]}"#,
        )
        .unwrap();
        assert_eq!(r.version, "0.4.2");
        assert_eq!(
            r.url,
            "https://github.com/jst7/ilixPatch/releases/tag/v0.4.2"
        );
        assert!(parse_latest(br#"{"message":"Not Found"}"#).is_err());
        assert!(parse_latest(br#"{"tag_name":"nightly","html_url":"x"}"#).is_err());
    }
}
