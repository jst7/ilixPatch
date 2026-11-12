//! Cloning with the system `git` (D26): live progress, cancellation and
//! HTTPS credentials passed only through the environment.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Error, Result, cli, invalid, io_err};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneOptions {
    pub url: String,
    /// Existing directory; the repo is cloned into `parent_dir/dir_name`.
    pub parent_dir: PathBuf,
    /// Defaults to the repo name derived from the URL.
    #[serde(default)]
    pub dir_name: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Shallow clone depth (implies `--single-branch`).
    #[serde(default)]
    pub depth: Option<u32>,
    /// HTTPS credentials, sent as an `Authorization` header via environment config.
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    /// Used by the app to register the clone as a project.
    #[serde(default)]
    pub add_as_project: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CloneEvent {
    Progress {
        phase: String,
        percent: Option<u8>,
        message: String,
    },
    Done {
        path: PathBuf,
    },
}

/// Cancels a running [`clone_repo`] from another thread.
#[derive(Clone, Default)]
pub struct CloneCancel(Arc<CancelInner>);

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    child: Mutex<Option<Child>>,
}

impl CloneCancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Kill the git process; [`clone_repo`] then removes the partial clone.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        if let Some(child) = self.lock().as_mut() {
            kill_tree(child);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Child>> {
        self.0.child.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Kill git and its helpers (remote-https, index-pack…), which share its
/// stderr pipe: killing only git could leave the pipe open.
fn kill_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(unix)]
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", pid.as_str(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("http://")
}

/// Accept https, http, ssh, file URLs, scp-like `user@host:path` and absolute
/// local paths. Rejects transport helpers (`ext::…`) and option-like values.
pub fn validate_url(url: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::Git(format!("invalid repository URL: {why}")));
    if url.is_empty() {
        return bad("empty");
    }
    if url.starts_with('-') {
        return bad("must not start with '-'");
    }
    if url.chars().any(char::is_control) {
        return bad("contains control characters");
    }
    if url.contains("::") {
        return bad("transport helpers (like ext::) are not allowed");
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        let ok = ["https", "http", "ssh", "file"].contains(&scheme.to_ascii_lowercase().as_str());
        return if !ok {
            bad("only https, http, ssh and file URLs are supported")
        } else if rest.is_empty() || rest.starts_with('-') {
            bad("missing host or path")
        } else {
            Ok(())
        };
    }
    if Path::new(url).is_absolute() {
        return Ok(());
    }
    // scp-like: user@host:path (host without '/').
    if let Some((user_host, path)) = url.split_once(':')
        && let Some((user, host)) = user_host.split_once('@')
        && !user.is_empty()
        && !host.is_empty()
        && !host.contains('/')
        && !host.starts_with('-')
        && !path.is_empty()
    {
        return Ok(());
    }
    bad("expected https://, ssh://, git@host:path, file:// or an absolute path")
}

/// The repo name from a URL: `https://h/org/repo.git/` → `repo`, `git@h:org/repo.git` → `repo`.
pub fn dir_name_from_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\', ':']).next()?;
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_string())
}

fn validate_dir_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.chars().any(char::is_control)
    {
        return Err(invalid(name, "invalid directory name"));
    }
    Ok(())
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Replace secrets (and passwords embedded in URLs) in text shown to the user.
fn scrub(text: &str, secrets: &[String]) -> String {
    let mut s = text.to_string();
    for secret in secrets.iter().filter(|x| !x.is_empty()) {
        s = s.replace(secret.as_str(), "***");
    }
    // https://user:pass@host → https://user:***@host
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find("://") {
        out.push_str(&rest[..i + 3]);
        rest = &rest[i + 3..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '/' || c == '\'')
            .unwrap_or(rest.len());
        let authority = &rest[..end];
        if let Some(at) = authority.rfind('@')
            && let Some(colon) = authority[..at].find(':')
        {
            out.push_str(&authority[..colon]);
            out.push_str(":***");
            out.push_str(&authority[at..]);
        } else {
            out.push_str(authority);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// A friendlier explanation for common clone failures, followed by git's message.
fn friendly(stderr: &str) -> String {
    let l = stderr.to_ascii_lowercase();
    let hint = if l.contains("host key verification failed") {
        Some(
            "SSH host key verification failed: the server isn't in your known_hosts. \
Connect once with `ssh` from a terminal to trust it, or use the HTTPS URL.",
        )
    } else if l.contains("permission denied (publickey") {
        Some(
            "SSH authentication failed: no SSH key accepted. Add your key to ssh-agent, or use HTTPS with a token.",
        )
    } else if l.contains("authentication failed")
        || l.contains("could not read username")
        || l.contains("could not read password")
        || l.contains("terminal prompts disabled")
        || l.contains("http basic: access denied")
        || l.contains("403")
        || l.contains("401")
    {
        Some(
            "Authentication failed. Enter a username and token, or set up a git credential helper.",
        )
    } else if l.contains("repository not found")
        || l.contains("not found")
        || l.contains("does not appear to be a git repository")
        || l.contains("does not exist")
    {
        Some("Repository not found, or you don't have access to it.")
    } else if l.contains("could not resolve host")
        || l.contains("failed to connect")
        || l.contains("connection timed out")
        || l.contains("connection refused")
        || l.contains("network is unreachable")
        || l.contains("could not resolve hostname")
    {
        Some("Network error: couldn't reach the server. Check the URL and your connection.")
    } else {
        None
    };
    match hint {
        Some(h) => format!("{h}\n{stderr}"),
        None => stderr.to_string(),
    }
}

/// `"Receiving objects:  45% (450/1000), 1.2 MiB | 2.0 MiB/s"` → event.
pub(crate) fn parse_progress(line: &str) -> Option<CloneEvent> {
    let line = line.trim();
    let body = line.strip_prefix("remote:").map(str::trim).unwrap_or(line);
    if body.is_empty() {
        return None;
    }
    if let Some(rest) = body.strip_prefix("Cloning into") {
        let _ = rest;
        return Some(CloneEvent::Progress {
            phase: "Cloning".into(),
            percent: None,
            message: body.to_string(),
        });
    }
    let (phase, rest) = body.split_once(':')?;
    if phase.is_empty() || phase.len() > 40 || !phase.chars().all(|c| c.is_alphabetic() || c == ' ')
    {
        return None;
    }
    let percent = rest
        .trim_start()
        .split_once('%')
        .and_then(|(n, _)| n.trim().parse::<u8>().ok())
        .map(|p| p.min(100));
    let lower = phase.to_ascii_lowercase();
    let counter = rest.trim_start().starts_with(|c: char| c.is_ascii_digit());
    if ["fatal", "error", "warning", "hint"].contains(&lower.trim())
        || (percent.is_none() && !counter)
    {
        return None;
    }
    Some(CloneEvent::Progress {
        phase: phase.trim().to_string(),
        percent,
        message: body.to_string(),
    })
}

fn cleanup(target: &Path, existed: bool) {
    if existed {
        if let Ok(entries) = fs::read_dir(target) {
            for e in entries.flatten() {
                let p = e.path();
                let _ = if p.is_dir() && !e.file_type().is_ok_and(|t| t.is_symlink()) {
                    fs::remove_dir_all(&p)
                } else {
                    fs::remove_file(&p)
                };
            }
        }
    } else if target.exists() {
        let _ = fs::remove_dir_all(target);
    }
}

/// Clone `opts.url` into `opts.parent_dir/<dir_name>`, streaming progress to
/// `on_event` (throttled) and ending with [`CloneEvent::Done`]. On failure or
/// cancellation the partial clone is removed. Returns the repo path.
pub fn clone_repo(
    opts: &CloneOptions,
    cancel: &CloneCancel,
    mut on_event: impl FnMut(CloneEvent),
) -> Result<PathBuf> {
    let url = opts.url.trim();
    validate_url(url)?;
    let name = match non_empty(&opts.dir_name) {
        Some(n) => n.to_string(),
        None => dir_name_from_url(url).ok_or_else(|| {
            Error::Git("can't derive a directory name from the URL; please enter one".into())
        })?,
    };
    validate_dir_name(&name)?;
    let parent = dunce::canonicalize(&opts.parent_dir).map_err(io_err(&opts.parent_dir))?;
    if !parent.is_dir() {
        return Err(invalid(parent, "not a directory"));
    }
    let target = parent.join(&name);
    let existed = match fs::symlink_metadata(&target) {
        Ok(m) if m.is_dir() => {
            if fs::read_dir(&target)
                .map_err(io_err(&target))?
                .next()
                .is_some()
            {
                return Err(invalid(target, "already exists and is not empty"));
            }
            true
        }
        Ok(_) => return Err(invalid(target, "already exists")),
        Err(_) => false,
    };

    let mut args: Vec<String> = vec![
        "-c".into(),
        "protocol.ext.allow=never".into(),
        "clone".into(),
        "--progress".into(),
    ];
    if let Some(b) = non_empty(&opts.branch) {
        crate::check_branch_name(b)?;
        args.extend(["--branch".into(), b.to_string()]);
    }
    if let Some(d) = opts.depth {
        if d == 0 {
            return Err(Error::Git("depth must be at least 1".into()));
        }
        args.extend(["--depth".into(), d.to_string(), "--single-branch".into()]);
    }
    args.extend([
        "--".into(),
        url.to_string(),
        target.to_string_lossy().into_owned(),
    ]);

    let mut cmd = cli::command(&parent, &args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // Own process group, so cancelling can kill git's helper processes too.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut secrets = Vec::new();
    let (user, token) = (non_empty(&opts.username), non_empty(&opts.token));
    if user.is_some() || token.is_some() {
        if !is_http(url) {
            return Err(Error::Git(
                "a username/token can only be used with https:// URLs".into(),
            ));
        }
        let user = user.unwrap_or("x-access-token");
        let encoded = base64(format!("{user}:{}", token.unwrap_or("")).as_bytes());
        // Config from the environment (git ≥ 2.31): not in argv, not on disk.
        cmd.env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Basic {encoded}"),
            );
        secrets.push(encoded);
        if let Some(t) = token {
            secrets.push(t.to_string());
        }
    }

    if cancel.is_cancelled() {
        return Err(Error::Git("clone cancelled".into()));
    }
    let mut child = cmd.spawn().map_err(cli::spawn_err)?;
    let mut stderr = child.stderr.take().expect("piped stderr");
    *cancel.lock() = Some(child);
    if cancel.is_cancelled() {
        cancel.cancel();
    }

    let mut buf = [0u8; 4096];
    let mut pending = Vec::new();
    let mut tail: Vec<String> = Vec::new();
    let mut last_emit: Option<(Instant, String)> = None;
    let mut handle_line = |line: &str, on_event: &mut dyn FnMut(CloneEvent)| {
        let line = scrub(line, &secrets);
        match parse_progress(&line) {
            Some(ev @ CloneEvent::Progress { .. }) => {
                let CloneEvent::Progress { phase, percent, .. } = &ev else {
                    unreachable!()
                };
                let now = Instant::now();
                let due = match &last_emit {
                    None => true,
                    Some((t, p)) => {
                        p != phase
                            || *percent == Some(100)
                            || now - *t >= Duration::from_millis(100)
                    }
                };
                if due {
                    last_emit = Some((now, phase.clone()));
                    on_event(ev);
                }
            }
            _ => {
                if !line.trim().is_empty() {
                    tail.push(line.trim().to_string());
                    if tail.len() > 20 {
                        tail.remove(0);
                    }
                }
            }
        }
    };
    loop {
        let n = match stderr.read(&mut buf) {
            Ok(0) | Err(_) => 0,
            Ok(n) => n,
        };
        if n == 0 {
            break;
        }
        pending.extend_from_slice(&buf[..n]);
        while let Some(i) = pending.iter().position(|b| *b == b'\r' || *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=i).collect();
            handle_line(String::from_utf8_lossy(&line[..i]).as_ref(), &mut on_event);
        }
    }
    if !pending.is_empty() {
        handle_line(String::from_utf8_lossy(&pending).as_ref(), &mut on_event);
    }
    let status = cancel.lock().take().map(|mut c| c.wait());

    if cancel.is_cancelled() {
        cleanup(&target, existed);
        return Err(Error::Git("clone cancelled".into()));
    }
    match status {
        Some(Ok(s)) if s.success() => {}
        Some(Ok(_)) => {
            cleanup(&target, existed);
            let msg = if tail.is_empty() {
                "git clone failed".to_string()
            } else {
                tail.join("\n")
            };
            return Err(Error::Git(friendly(&msg)));
        }
        Some(Err(e)) => {
            cleanup(&target, existed);
            return Err(Error::Git(format!("git clone failed: {e}")));
        }
        None => return Err(Error::Git("clone cancelled".into())),
    }
    on_event(CloneEvent::Done {
        path: target.clone(),
    });
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        for ok in [
            "https://github.com/org/repo.git",
            "http://host/repo",
            "ssh://git@host:22/org/repo.git",
            "git@github.com:org/repo.git",
            "file:///tmp/repo.git",
            // An absolute local path ("/tmp/…" is not absolute on Windows).
            if cfg!(windows) {
                r"C:\repos\repo.git"
            } else {
                "/tmp/repo.git"
            },
        ] {
            assert!(validate_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-uevil",
            "--upload-pack=touch /tmp/x",
            "ext::sh -c touch% /tmp/pwned",
            "fd::17",
            "ftp://host/repo",
            "git://host/repo",
            "relative/path",
            "host:path",
            "https://",
            "https://host/a\nb",
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dir_names() {
        assert_eq!(
            dir_name_from_url("https://h/org/repo.git/").as_deref(),
            Some("repo")
        );
        assert_eq!(
            dir_name_from_url("git@h:org/my-api.git").as_deref(),
            Some("my-api")
        );
        assert_eq!(dir_name_from_url("git@h:repo").as_deref(), Some("repo"));
        assert_eq!(dir_name_from_url("/tmp/x/").as_deref(), Some("x"));
        assert_eq!(dir_name_from_url("https://h/"), Some("h".into()));
        assert_eq!(dir_name_from_url("https://h/.git"), None);
        assert!(validate_dir_name("a/b").is_err());
        assert!(validate_dir_name("..").is_err());
        assert!(validate_dir_name("ok-name").is_ok());
    }

    #[test]
    fn base64_and_scrub() {
        assert_eq!(base64(b"user:tok"), "dXNlcjp0b2s=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(
            scrub(
                "fatal: https://me:s3cret@host/x failed s3cret",
                &["s3cret".into()]
            ),
            "fatal: https://me:***@host/x failed ***"
        );
        assert_eq!(scrub("see https://u:p@h/r", &[]), "see https://u:***@h/r");
    }

    #[test]
    fn progress_lines() {
        let ev = parse_progress("Receiving objects:  45% (450/1000), 1.2 MiB | 2.0 MiB/s").unwrap();
        assert_eq!(
            ev,
            CloneEvent::Progress {
                phase: "Receiving objects".into(),
                percent: Some(45),
                message: "Receiving objects:  45% (450/1000), 1.2 MiB | 2.0 MiB/s".into()
            }
        );
        let CloneEvent::Progress { phase, percent, .. } =
            parse_progress("remote: Counting objects: 100% (3/3), done.").unwrap()
        else {
            panic!()
        };
        assert_eq!((phase.as_str(), percent), ("Counting objects", Some(100)));
        assert!(matches!(
            parse_progress("Cloning into 'repo'..."),
            Some(CloneEvent::Progress { percent: None, .. })
        ));
        assert!(parse_progress("fatal: repository 'x' not found").is_none());
        assert!(
            parse_progress("warning: You appear to have cloned an empty repository.").is_none()
        );
    }
}
