//! Persistence (L2): the workspace journal.
//!
//! The server's source of truth is an append-only log of [`Sequenced`] events, one JSON object
//! per line in `events.jsonl`. Appends are flushed and synced before the hub acknowledges the
//! request, so an acknowledged message survives a crash. Loading is tolerant: a torn last line
//! (power lost mid-write) or a corrupt line is skipped and reported, never silently repaired,
//! and the file itself is left as it was.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chatcraft_model::Sequenced;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("encode: {0}")]
    Encode(String),
}

/// Somewhere events are kept.
pub trait Journal: Send {
    /// Appends events durably; returns once they are safe.
    fn append(&mut self, events: &[Sequenced]) -> Result<(), StoreError>;
}

/// Keeps nothing (the in-process demo workspace).
#[derive(Default)]
pub struct NullJournal;

impl Journal for NullJournal {
    fn append(&mut self, _: &[Sequenced]) -> Result<(), StoreError> {
        Ok(())
    }
}

/// Keeps events in memory (tests).
#[derive(Default)]
pub struct MemJournal(pub Vec<Sequenced>);

impl Journal for MemJournal {
    fn append(&mut self, events: &[Sequenced]) -> Result<(), StoreError> {
        self.0.extend_from_slice(events);
        Ok(())
    }
}

/// What loading found.
#[derive(Debug, Default)]
pub struct Loaded {
    pub events: Vec<Sequenced>,
    /// Lines that were skipped, with the reason (shown to the operator).
    pub warnings: Vec<String>,
}

/// `events.jsonl` in a data directory.
pub struct FileJournal {
    path: PathBuf,
    file: File,
}

impl FileJournal {
    pub const FILE: &'static str = "events.jsonl";

    /// Opens (creating if needed) the journal in `dir` and loads what it holds.
    pub fn open(dir: &Path) -> Result<(FileJournal, Loaded), StoreError> {
        std::fs::create_dir_all(dir).map_err(|source| StoreError::Io { path: dir.to_path_buf(), source })?;
        let path = dir.join(Self::FILE);
        let loaded = if path.exists() { load(&path)? } else { Loaded::default() };
        let mut file = OpenOptions::new().create(true).append(true).open(&path).map_err(|source| StoreError::Io { path: path.clone(), source })?;
        // A torn last line must not swallow the next event: start appends on a fresh line.
        if ends_without_newline(&path) {
            file.write_all(b"\n").map_err(|source| StoreError::Io { path: path.clone(), source })?;
        }
        Ok((FileJournal { path, file }, loaded))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn ends_without_newline(path: &Path) -> bool {
    std::fs::read(path).ok().and_then(|b| b.last().copied()).is_some_and(|b| b != b'\n')
}

impl Journal for FileJournal {
    fn append(&mut self, events: &[Sequenced]) -> Result<(), StoreError> {
        let mut buf = String::new();
        for e in events {
            buf.push_str(&serde_json::to_string(e).map_err(|e| StoreError::Encode(e.to_string()))?);
            buf.push('\n');
        }
        let io = |source| StoreError::Io { path: self.path.clone(), source };
        self.file.write_all(buf.as_bytes()).map_err(io)?;
        self.file.flush().map_err(io)?;
        self.file.sync_data().map_err(io)
    }
}

/// Reads a journal file. Sequence numbers must increase; other lines are skipped with a warning.
pub fn load(path: &Path) -> Result<Loaded, StoreError> {
    let f = File::open(path).map_err(|source| StoreError::Io { path: path.to_path_buf(), source })?;
    let mut out = Loaded::default();
    let mut last = 0u64;
    for (n, line) in BufReader::new(f).lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                out.warnings.push(format!("line {}: unreadable ({e})", n + 1));
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Sequenced>(&line) {
            Ok(s) if s.seq > last => {
                last = s.seq;
                out.events.push(s);
            }
            Ok(s) => out.warnings.push(format!("line {}: sequence {} after {last}, skipped", n + 1, s.seq)),
            Err(e) => out.warnings.push(format!("line {}: {e}, skipped", n + 1)),
        }
    }
    for w in &out.warnings {
        log::warn!("{}: {w}", path.display());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use chatcraft_model::{Event, Ts};

    use super::*;

    fn ev(seq: u64) -> Sequenced {
        Sequenced { seq, event: Event::WorkspaceCreated { name: format!("w{seq}"), at: Ts(0) } }
    }

    #[test]
    fn appends_survive_reopen_and_torn_lines_are_skipped() {
        let dir = std::env::temp_dir().join(format!("chatcraft-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let (mut j, l) = FileJournal::open(&dir).unwrap();
            assert!(l.events.is_empty());
            j.append(&[ev(1), ev(2)]).unwrap();
        }
        // Simulate a crash mid-write.
        let mut f = OpenOptions::new().append(true).open(dir.join(FileJournal::FILE)).unwrap();
        f.write_all(b"{\"seq\":3,\"event\":{\"ty").unwrap();
        drop(f);
        {
            let (mut j, l) = FileJournal::open(&dir).unwrap();
            assert_eq!(l.events.len(), 2);
            assert_eq!(l.warnings.len(), 1);
            j.append(&[ev(3)]).unwrap();
        }
        let (_, l) = FileJournal::open(&dir).unwrap();
        assert_eq!(l.events.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
