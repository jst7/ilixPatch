//! The list of registered projects, stored as `projects.toml` in the app
//! config dir. Only paths are stored; projects themselves live on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    paths: Vec<PathBuf>,
}

pub struct Registry {
    file: PathBuf,
    lock: Mutex<()>,
}

impl Registry {
    pub fn new(file: PathBuf) -> Self {
        Self {
            file,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<Vec<PathBuf>, String> {
        match fs::read_to_string(&self.file) {
            Ok(s) => toml::from_str::<RegistryFile>(&s)
                .map(|f| f.paths)
                .map_err(|e| format!("{}: {e}", self.file.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("{}: {e}", self.file.display())),
        }
    }

    fn write(&self, paths: Vec<PathBuf>) -> Result<(), String> {
        let err = |e: &dyn std::fmt::Display| format!("{}: {e}", self.file.display());
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir).map_err(|e| err(&e))?;
        }
        let s = toml::to_string_pretty(&RegistryFile { paths }).map_err(|e| err(&e))?;
        fs::write(&self.file, s).map_err(|e| err(&e))
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Registered project roots, in insertion order.
    pub fn paths(&self) -> Result<Vec<PathBuf>, String> {
        let _g = self.guard();
        self.read()
    }

    /// Append `path` unless already registered.
    pub fn add(&self, path: &Path) -> Result<(), String> {
        let _g = self.guard();
        let mut paths = self.read()?;
        if !paths.iter().any(|p| p == path) {
            paths.push(path.to_path_buf());
            self.write(paths)?;
        }
        Ok(())
    }

    /// Unregister `path` (matched as given or canonicalized). Never touches files.
    pub fn remove(&self, path: &Path) -> Result<(), String> {
        let _g = self.guard();
        let canonical = fs::canonicalize(path).ok();
        let mut paths = self.read()?;
        let before = paths.len();
        paths.retain(|p| p != path && Some(p) != canonical.as_ref());
        if paths.len() != before {
            self.write(paths)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedupes_and_remove_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registry::new(dir.path().join("sub/projects.toml"));
        assert!(reg.paths().unwrap().is_empty());
        for p in ["/a", "/b", "/a", "/c"] {
            reg.add(Path::new(p)).unwrap();
        }
        assert_eq!(
            reg.paths().unwrap(),
            [PathBuf::from("/a"), "/b".into(), "/c".into()]
        );
        reg.remove(Path::new("/b")).unwrap();
        assert_eq!(reg.paths().unwrap(), [PathBuf::from("/a"), "/c".into()]);
    }
}
