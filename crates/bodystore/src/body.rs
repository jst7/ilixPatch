//! A response body kept in memory or in a temp file, and the sink that
//! decides where it goes while it is being received.

use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::{NamedTempFile, TempPath};

/// Prefix of the temp files created here (used to clean orphans).
pub const TEMP_PREFIX: &str = "ilixpatch-body-";

/// A received body. Cheap to clone; a file body is deleted when the last
/// clone is dropped.
#[derive(Debug, Clone)]
pub enum Body {
    Memory(Arc<[u8]>),
    File { path: Arc<TempPath>, len: u64 },
}

impl Default for Body {
    fn default() -> Self {
        Body::Memory(Arc::from(Vec::new()))
    }
}

impl From<Vec<u8>> for Body {
    fn from(v: Vec<u8>) -> Self {
        Body::Memory(Arc::from(v))
    }
}

impl Body {
    pub fn len(&self) -> u64 {
        match self {
            Body::Memory(b) => b.len() as u64,
            Body::File { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Body::File { .. })
    }

    /// The temp file, for file bodies.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Body::File { path, .. } => Some(path),
            Body::Memory(_) => None,
        }
    }

    /// A reader over the whole body.
    pub fn reader(&self) -> io::Result<Box<dyn Read + Send + '_>> {
        Ok(match self {
            Body::Memory(b) => Box::new(io::Cursor::new(&b[..])),
            Body::File { path, .. } => Box::new(io::BufReader::new(File::open(&**path)?)),
        })
    }

    /// Up to `length` bytes from `offset` (fewer at EOF).
    pub fn read_range(&self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        let len = self.len();
        if offset >= len {
            return Ok(Vec::new());
        }
        let n = length.min((len - offset) as usize);
        match self {
            Body::Memory(b) => Ok(b[offset as usize..offset as usize + n].to_vec()),
            Body::File { path, .. } => {
                let mut f = File::open(&**path)?;
                f.seek(SeekFrom::Start(offset))?;
                let mut out = vec![0; n];
                f.read_exact(&mut out)?;
                Ok(out)
            }
        }
    }

    /// The first `n` bytes.
    pub fn preview(&self, n: usize) -> io::Result<Vec<u8>> {
        self.read_range(0, n)
    }

    /// The body, at most `limit` bytes; `true` when it was cut.
    pub fn to_bytes_limited(&self, limit: usize) -> io::Result<(Vec<u8>, bool)> {
        Ok((self.preview(limit)?, self.len() > limit as u64))
    }

    /// Lossy UTF-8 text of at most `limit` bytes (cut on a character boundary).
    pub fn text_limited(&self, limit: usize) -> io::Result<(String, bool)> {
        let (mut bytes, truncated) = self.to_bytes_limited(limit)?;
        if truncated {
            bytes.truncate(utf8_floor(&bytes));
        }
        Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
    }

    /// Stream the body to `path` (no full copy in memory). Returns bytes written.
    pub fn copy_to(&self, path: &Path) -> io::Result<u64> {
        let mut out = BufWriter::new(File::create(path)?);
        let n = io::copy(&mut self.reader()?, &mut out)?;
        out.flush()?;
        Ok(n)
    }
}

/// Length of the longest prefix of `bytes` that does not end inside a UTF-8
/// sequence (invalid bytes are left for the lossy decoder).
pub fn utf8_floor(bytes: &[u8]) -> usize {
    let n = bytes.len();
    // Look back at most 3 bytes for the start of the last sequence.
    for back in 1..=3.min(n) {
        let b = bytes[n - back];
        if b & 0b1100_0000 == 0b1000_0000 {
            continue; // continuation byte
        }
        let need = if b >= 0xF8 {
            1 // not a valid lead byte
        } else if b >= 0xF0 {
            4
        } else if b >= 0xE0 {
            3
        } else if b >= 0xC0 {
            2
        } else {
            1
        };
        return if need > back { n - back } else { n };
    }
    n
}

/// Where a body goes while it is received.
#[derive(Debug, Clone, Default)]
pub struct SinkConfig {
    /// Bodies up to this size stay in memory.
    pub inline_limit: usize,
    /// Stop after this many bytes (`complete = false`).
    pub max_bytes: Option<u64>,
    /// Directory for temp files (system temp when `None`).
    pub dir: Option<PathBuf>,
    /// Count the bytes only (load tests, runs that don't need the body).
    pub discard: bool,
}

/// Receives chunks; keeps them in memory up to `inline_limit`, then spills to
/// a temp file. Dropping the sink deletes a partial temp file.
pub struct BodySink {
    cfg: SinkConfig,
    mem: Vec<u8>,
    file: Option<BufWriter<NamedTempFile>>,
    len: u64,
    truncated: bool,
}

impl BodySink {
    pub fn new(cfg: SinkConfig) -> Self {
        Self {
            cfg,
            mem: Vec::new(),
            file: None,
            len: 0,
            truncated: false,
        }
    }

    /// Bytes accepted so far.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Add a chunk. Returns `false` when the size limit was reached (stop reading).
    pub fn push(&mut self, chunk: &[u8]) -> io::Result<bool> {
        let mut chunk = chunk;
        let mut more = true;
        if let Some(max) = self.cfg.max_bytes {
            let room = max.saturating_sub(self.len);
            if chunk.len() as u64 > room {
                chunk = &chunk[..room as usize];
                self.truncated = true;
                more = false;
            }
        }
        self.len += chunk.len() as u64;
        if self.cfg.discard {
            return Ok(more);
        }
        if let Some(f) = &mut self.file {
            f.write_all(chunk)?;
        } else if self.mem.len() + chunk.len() > self.cfg.inline_limit {
            let tmp = match &self.cfg.dir {
                Some(dir) => {
                    std::fs::create_dir_all(dir)?;
                    tempfile::Builder::new()
                        .prefix(TEMP_PREFIX)
                        .tempfile_in(dir)?
                }
                None => tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile()?,
            };
            let mut f = BufWriter::with_capacity(256 * 1024, tmp);
            f.write_all(&self.mem)?;
            f.write_all(chunk)?;
            self.mem = Vec::new();
            self.file = Some(f);
        } else {
            self.mem.extend_from_slice(chunk);
        }
        Ok(more)
    }

    /// The body and whether it is complete (the size limit was not hit).
    pub fn finish(self) -> io::Result<(Body, bool)> {
        let complete = !self.truncated;
        let body = match self.file {
            Some(f) => {
                let tmp = f.into_inner().map_err(|e| e.into_error())?;
                Body::File {
                    path: Arc::new(tmp.into_temp_path()),
                    len: self.len,
                }
            }
            None => Body::from(self.mem),
        };
        Ok((body, complete))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_floor_cuts_incomplete_sequences() {
        let s = "aé€😀".as_bytes(); // 1 + 2 + 3 + 4 bytes
        assert_eq!(utf8_floor(s), s.len());
        assert_eq!(utf8_floor(&s[..s.len() - 1]), 6);
        assert_eq!(utf8_floor(&s[..5]), 3);
        assert_eq!(utf8_floor(&s[..2]), 1);
        assert_eq!(utf8_floor(b""), 0);
        assert_eq!(utf8_floor(&[0xff]), 1);
    }

    #[test]
    fn sink_spills_and_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = SinkConfig {
            inline_limit: 10,
            max_bytes: Some(25),
            dir: Some(dir.path().to_path_buf()),
            discard: false,
        };
        let mut s = BodySink::new(cfg.clone());
        assert!(s.push(b"0123456789").unwrap());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(s.push(b"abcdefghij").unwrap());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(!s.push(b"KLMNOPQRST").unwrap());
        let (body, complete) = s.finish().unwrap();
        assert!(!complete && body.is_file());
        assert_eq!(body.len(), 25);
        assert_eq!(body.read_range(18, 100).unwrap(), b"ijKLMNO");
        assert_eq!(body.to_bytes_limited(4).unwrap(), (b"0123".to_vec(), true));
        let copy = dir.path().join("copy.bin");
        assert_eq!(body.copy_to(&copy).unwrap(), 25);
        let path = body.path().unwrap().to_path_buf();
        drop(body);
        assert!(!path.exists(), "temp file deleted with the last clone");

        // Dropping a sink mid-way deletes its temp file.
        let mut s = BodySink::new(SinkConfig {
            max_bytes: None,
            ..cfg.clone()
        });
        s.push(&[0; 100]).unwrap();
        drop(s);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1); // only copy.bin

        let mut d = BodySink::new(SinkConfig {
            discard: true,
            ..cfg
        });
        d.push(&[0; 20]).unwrap();
        let (b, _) = d.finish().unwrap();
        assert!(b.is_empty());
    }
}
