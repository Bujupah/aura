//! Saved meetings.
//!
//! Each meeting is one file holding its transcript, topic notes and summary,
//! encrypted with AES-256-GCM. The key is supplied by the caller — on macOS
//! it lives in the Keychain — so a copy of the folder alone reveals nothing.
//! Audio is never part of a saved meeting.

use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aura_core::summary::MeetingSummary;
use aura_core::topics::Topic;
use aura_core::transcript::Turn;
use serde::{Deserialize, Serialize};

pub const KEY_BYTES: usize = 32;
const EXTENSION: &str = "aura";
/// Identifies the file format, so a future change can be told apart.
const MAGIC: &[u8; 6] = b"AURA\x00\x01";
const NONCE_BYTES: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("the saved sessions could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("a saved session is damaged or was written with a different key")]
    Unreadable,
    #[error("a saved session has contents this version does not understand")]
    Format(#[from] serde_json::Error),
    #[error("that is not a valid session id")]
    InvalidId,
}

/// A whole saved meeting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub id: String,
    pub title: String,
    /// Milliseconds since the Unix epoch.
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub turns: Vec<Turn>,
    pub topics: Vec<Topic>,
    pub summary: Option<MeetingSummary>,
}

/// What a list of sessions shows, without the content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub turn_count: usize,
    pub topic_count: usize,
    /// From the first turn to the last, in meeting time.
    pub duration_ms: u64,
    pub has_summary: bool,
}

impl SavedSession {
    pub fn meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.id.clone(),
            title: self.title.clone(),
            started_at_ms: self.started_at_ms,
            updated_at_ms: self.updated_at_ms,
            turn_count: self.turns.len(),
            topic_count: self.topics.len(),
            duration_ms: self.turns.iter().map(|turn| turn.end_ms).max().unwrap_or(0),
            has_summary: self.summary.is_some(),
        }
    }
}

/// A fresh random key for a new store.
pub fn generate_key() -> [u8; KEY_BYTES] {
    Aes256Gcm::generate_key(OsRng).into()
}

pub struct SessionStore {
    directory: PathBuf,
    cipher: Aes256Gcm,
}

impl SessionStore {
    /// Opens the store in `directory`, creating it if needed.
    pub fn open(directory: impl Into<PathBuf>, key: &[u8; KEY_BYTES]) -> Result<Self, StorageError> {
        let directory = directory.into();
        fs::create_dir_all(&directory)?;
        Ok(Self {
            directory,
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)),
        })
    }

    /// Writes the session, replacing any earlier version of it. The file is
    /// swapped in whole, so a crash mid-write cannot leave half a session.
    pub fn save(&self, session: &SavedSession) -> Result<(), StorageError> {
        let path = self.path(&session.id)?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let sealed = self
            .cipher
            .encrypt(&nonce, serde_json::to_vec(session)?.as_slice())
            .map_err(|_| StorageError::Unreadable)?;
        let mut contents = Vec::with_capacity(MAGIC.len() + NONCE_BYTES + sealed.len());
        contents.extend_from_slice(MAGIC);
        contents.extend_from_slice(&nonce);
        contents.extend_from_slice(&sealed);

        let partial = path.with_extension("partial");
        fs::write(&partial, contents)?;
        fs::rename(&partial, &path)?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> Result<SavedSession, StorageError> {
        self.read(&self.path(id)?)
    }

    /// Removes a session for good. Removing one that is already gone is fine.
    pub fn delete(&self, id: &str) -> Result<(), StorageError> {
        match fs::remove_file(self.path(id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Every readable session, most recently updated first. A file that
    /// cannot be read is skipped rather than hiding the others.
    pub fn list(&self) -> Result<Vec<SessionMeta>, StorageError> {
        let mut sessions: Vec<SessionMeta> = fs::read_dir(&self.directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == EXTENSION))
            .filter_map(|path| self.read(&path).ok())
            .map(|session| session.meta())
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at_ms));
        Ok(sessions)
    }

    fn read(&self, path: &Path) -> Result<SavedSession, StorageError> {
        let contents = fs::read(path)?;
        let sealed = contents.strip_prefix(MAGIC.as_slice()).ok_or(StorageError::Unreadable)?;
        if sealed.len() < NONCE_BYTES {
            return Err(StorageError::Unreadable);
        }
        let (nonce, sealed) = sealed.split_at(NONCE_BYTES);
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), sealed)
            .map_err(|_| StorageError::Unreadable)?;
        Ok(serde_json::from_slice(&plain)?)
    }

    /// Ids come from the interface, so they are confined to plain names and
    /// can never point outside the store.
    fn path(&self, id: &str) -> Result<PathBuf, StorageError> {
        let plain = !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !plain {
            return Err(StorageError::InvalidId);
        }
        Ok(self.directory.join(format!("{id}.{EXTENSION}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::transcript::Speaker;

    const KEY: [u8; KEY_BYTES] = [7; KEY_BYTES];

    fn session(id: &str, updated_at_ms: u64) -> SavedSession {
        SavedSession {
            id: id.into(),
            title: "CMDB accuracy call".into(),
            started_at_ms: 1_000,
            updated_at_ms,
            turns: vec![Turn {
                id: "customer-0".into(),
                speaker: Speaker::Customer,
                text: "Our CMDB gets outdated very quickly.".into(),
                start_ms: 800,
                end_ms: 4_200,
                is_final: true,
                translation: None,
            }],
            topics: Vec::new(),
            summary: None,
        }
    }

    #[test]
    fn a_session_survives_a_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::open(directory.path(), &KEY).unwrap();
        let saved = session("s-1", 5_000);
        store.save(&saved).unwrap();
        assert_eq!(store.load("s-1").unwrap(), saved);
        assert_eq!(store.load("s-1").unwrap().meta().duration_ms, 4_200);
    }

    #[test]
    fn nothing_readable_is_left_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::open(directory.path(), &KEY).unwrap();
        store.save(&session("s-1", 5_000)).unwrap();
        let raw = fs::read(directory.path().join("s-1.aura")).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("CMDB") && !text.contains("customer"), "plaintext leaked into the file");
    }

    #[test]
    fn the_wrong_key_or_a_damaged_file_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        SessionStore::open(directory.path(), &KEY).unwrap().save(&session("s-1", 5_000)).unwrap();

        let other = SessionStore::open(directory.path(), &[9; KEY_BYTES]).unwrap();
        assert!(matches!(other.load("s-1"), Err(StorageError::Unreadable)));

        let path = directory.path().join("s-1.aura");
        let mut raw = fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        fs::write(&path, raw).unwrap();
        let store = SessionStore::open(directory.path(), &KEY).unwrap();
        assert!(matches!(store.load("s-1"), Err(StorageError::Unreadable)));
    }

    #[test]
    fn saving_again_replaces_and_the_list_is_newest_first() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::open(directory.path(), &KEY).unwrap();
        store.save(&session("old", 1_000)).unwrap();
        store.save(&session("new", 9_000)).unwrap();
        store.save(&SavedSession { title: "Renamed".into(), ..session("old", 2_000) }).unwrap();
        // A stray or unreadable file does not hide the real sessions.
        fs::write(directory.path().join("junk.aura"), b"not a session").unwrap();

        let listed = store.list().unwrap();
        assert_eq!(listed.iter().map(|meta| meta.id.as_str()).collect::<Vec<_>>(), ["new", "old"]);
        assert_eq!(listed[1].title, "Renamed");
        assert_eq!(listed[1].turn_count, 1);
    }

    #[test]
    fn deleting_removes_the_file_and_is_safe_to_repeat() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::open(directory.path(), &KEY).unwrap();
        store.save(&session("s-1", 5_000)).unwrap();
        store.delete("s-1").unwrap();
        store.delete("s-1").unwrap();
        assert!(store.list().unwrap().is_empty());
        assert!(matches!(store.load("s-1"), Err(StorageError::Io(_))));
    }

    #[test]
    fn an_id_cannot_reach_outside_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::open(directory.path(), &KEY).unwrap();
        for id in ["../escape", "a/b", "", "with space", ".hidden"] {
            assert!(matches!(store.load(id), Err(StorageError::InvalidId)), "{id:?} was accepted");
            assert!(matches!(store.delete(id), Err(StorageError::InvalidId)));
        }
    }
}
