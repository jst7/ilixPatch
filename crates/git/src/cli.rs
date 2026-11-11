//! Running the system `git` binary.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::{Error, Result};

/// `ILIXPATCH_GIT` overrides the binary (useful when the GUI app's PATH is minimal).
fn git_bin() -> OsString {
    std::env::var_os("ILIXPATCH_GIT").unwrap_or_else(|| "git".into())
}

pub(crate) fn command<I, S>(dir: &Path, args: I) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = Command::new(git_bin());
    cmd.current_dir(dir)
        .args(args)
        // Never wait for input on a terminal (credentials, passphrases).
        .env("GIT_TERMINAL_PROMPT", "0")
        // Don't take the index lock just to refresh stat info (status).
        .env("GIT_OPTIONAL_LOCKS", "0")
        // Paths are passed literally, never as globs.
        .env("GIT_LITERAL_PATHSPECS", "1")
        // Operate on `dir`, not on some repo inherited from our environment.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub(crate) fn spawn_err(e: io::Error) -> Error {
    if e.kind() == io::ErrorKind::NotFound {
        Error::GitNotFound
    } else {
        Error::Git(format!("failed to run git: {e}"))
    }
}

fn finish(out: std::process::Output) -> Result<Vec<u8>> {
    if out.status.success() {
        return Ok(out.stdout);
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let msg = if stderr.is_empty() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if stdout.is_empty() {
            format!("git exited with {}", out.status)
        } else {
            stdout
        }
    } else {
        stderr
    };
    Err(Error::Git(msg))
}

/// Run git in `dir`; stdout on success, stderr as the error otherwise.
pub(crate) fn git<I, S>(dir: &Path, args: I) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = command(dir, args)
        .stdin(Stdio::null())
        .output()
        .map_err(spawn_err)?;
    finish(out)
}

/// Like [`git`], with extra environment variables (e.g. `GIT_EDITOR=true`).
pub(crate) fn git_env<I, S>(dir: &Path, args: I, env: &[(&str, &str)]) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut cmd = command(dir, args);
    cmd.stdin(Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    finish(cmd.output().map_err(spawn_err)?)
}

/// Like [`git`], feeding `input` on stdin (used for `--pathspec-from-file=-`).
pub(crate) fn git_stdin<I, S>(dir: &Path, args: I, input: Vec<u8>) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = command(dir, args)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(spawn_err)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    // Write from another thread so a chatty child can't deadlock us.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().map_err(spawn_err)?;
    let _ = writer.join();
    finish(out)
}
