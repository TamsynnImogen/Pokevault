//! Recoverable publication to the save file explicitly chosen by the user.
//!
//! SQLite and a battery file cannot share an atomic commit. We therefore persist
//! both images and an intent before replacing the file, then let SQLite commit.
//! Recovery compares the actual database revision with that intent: committed
//! transfers keep the new file; rolled-back transfers restore the old image.
//! Unexpected external edits are preserved and block further transfers.

use crate::{
    sha256,
    store::{ManagedSave, ManagedStore},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// A chosen file has one stable managed identity even as its history advances.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveLink {
    pub schema: u32,
    pub save_id: i64,
    pub path: PathBuf,
    pub profile_sha256: String,
    pub file_sha256: String,
}

/// Complete recovery information, written durably before touching the save.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    link: SaveLink,
    before_revision: i64,
    before_sha256: String,
    after_sha256: String,
    before_backup: String,
    after_backup: String,
}

/// The lock excludes cooperating Vault processes, not external emulators. Users
/// must close their emulator; hash checks detect changes made since Choose save.
pub struct LinkedSaves {
    directory: PathBuf,
    backups: PathBuf,
    _lock: fs::File,
}

impl LinkedSaves {
    pub fn open(workspace: &Path) -> Result<Self> {
        let directory = owned_directory(&workspace.join("linked-saves"))?;
        let backups = owned_directory(&directory.join("backups"))?;
        let lock_path = directory.join("workspace.lock");
        if lock_path.exists() || lock_path.is_symlink() {
            ensure!(
                fs::symlink_metadata(&lock_path)?.is_file() && !lock_path.is_symlink(),
                "invalid save-link lock file"
            );
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock().context(
            "this workspace is already open in another Vault process; close its window first",
        )?;
        Ok(Self {
            directory,
            backups,
            _lock: lock,
        })
    }

    fn link_path(&self, id: i64) -> PathBuf {
        self.directory.join(format!("save-{id}.link.json"))
    }

    pub fn link(&self, id: i64) -> Result<Option<SaveLink>> {
        let path = self.link_path(id);
        if !path.try_exists()? {
            return Ok(None);
        }
        let link: SaveLink = serde_json::from_slice(&read_regular(&path, 64 * 1024)?)?;
        ensure!(
            link.schema == 1 && link.save_id == id && link.path.is_absolute(),
            "invalid save link"
        );
        Ok(Some(link))
    }

    /// Look up by canonical path, not by ROM hash or changing save content.
    pub fn find_path(&self, path: &Path) -> Result<Option<SaveLink>> {
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = name
                .strip_prefix("save-")
                .and_then(|s| s.strip_suffix(".link.json"))
                .and_then(|s| s.parse::<i64>().ok())
            {
                if let Some(link) = self.link(id)? {
                    if link.path == path {
                        return Ok(Some(link));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Called only after explicit Choose save and verified engine/profile parsing.
    pub fn bind(&self, path: PathBuf, save: &ManagedSave) -> Result<()> {
        ensure!(
            !self.pending(),
            "a previous transfer needs recovery before choosing a save"
        );
        let bytes = read_regular(&path, 128 * 1024)?;
        ensure!(
            bytes == save.bytes,
            "chosen save changed while loading; choose it again after closing the emulator"
        );
        let link = SaveLink {
            schema: 1,
            save_id: save.id,
            path,
            profile_sha256: save.profile.fingerprint(),
            file_sha256: sha256(&bytes),
        };
        atomic_write(&self.link_path(save.id), &serde_json::to_vec_pretty(&link)?)
    }

    fn intent_path(&self) -> PathBuf {
        self.directory.join("pending.json")
    }

    pub fn pending(&self) -> bool {
        self.intent_path().exists()
    }

    /// Read-only status lets the interface display conflicts without hiding data.
    pub fn status(&self, save: &ManagedSave) -> Result<(&'static str, Option<PathBuf>)> {
        let Some(link) = self.link(save.id)? else {
            return Ok(("unlinked", None));
        };
        let status = if self.pending() {
            "pending"
        } else if link.profile_sha256 != save.profile.fingerprint()
            || read_regular(&link.path, 128 * 1024)
                .map(|b| sha256(&b) != link.file_sha256 || b != save.bytes)
                .unwrap_or(true)
        {
            "changed"
        } else {
            "linked"
        };
        Ok((status, Some(link.path)))
    }

    /// Refuse stale selections and external gameplay changes before any SQL work.
    pub fn preflight(&self, save: &ManagedSave) -> Result<()> {
        let (status, _) = self.status(save)?;
        ensure!(status == "linked", "chosen save is {status}; close the emulator and use Choose save again before transferring");
        Ok(())
    }

    /// Invoked by the store while its immediate SQL transaction is still open.
    pub fn publish(&self, id: i64, revision: i64, before: &[u8], after: &[u8]) -> Result<()> {
        ensure!(
            !self.pending(),
            "an earlier save publication needs recovery"
        );
        let link = self.link(id)?.context("save is not linked")?;
        let before_sha256 = sha256(before);
        ensure!(
            link.file_sha256 == before_sha256,
            "linked save does not match the selected revision"
        );
        ensure!(
            read_regular(&link.path, 128 * 1024)? == before,
            "save changed outside the Vault; close the emulator and Choose save again"
        );
        let token = Uuid::new_v4();
        let before_backup = format!("save-{id}-revision-{revision}-{token}-before.sav");
        let after_backup = format!("save-{id}-revision-{revision}-{token}-after.sav");
        write_new(&self.backups.join(&before_backup), before)?;
        write_new(&self.backups.join(&after_backup), after)?;
        sync_directory(&self.backups)?;
        let intent = Intent {
            link,
            before_revision: revision,
            before_sha256,
            after_sha256: sha256(after),
            before_backup,
            after_backup,
        };
        atomic_write(&self.intent_path(), &serde_json::to_vec_pretty(&intent)?)?;
        // Recheck immediately before replacement. A non-cooperating running
        // emulator can still race this check, hence the explicit closed-emulator rule.
        replace_save(&intent.link.path, &intent.before_sha256, after)
    }

    /// Resolve the only two normal crash outcomes without guessing which save
    /// the user intended. A third-party edit is never overwritten by recovery.
    pub fn recover(&self, store: &ManagedStore) -> Result<()> {
        if !self.pending() {
            return Ok(());
        }
        let intent: Intent =
            serde_json::from_slice(&read_regular(&self.intent_path(), 64 * 1024)?)?;
        let save = store.save(intent.link.save_id)?;
        ensure!(
            intent.link.schema == 1 && intent.link.profile_sha256 == save.profile.fingerprint(),
            "pending transfer profile mismatch"
        );
        let before = self.backup(&intent.before_backup, &intent.before_sha256)?;
        let after = self.backup(&intent.after_backup, &intent.after_sha256)?;
        let actual = sha256(&read_regular(&intent.link.path, 128 * 1024)?);
        if save.revision == intent.before_revision && save.bytes == before {
            if actual == intent.after_sha256 {
                replace_save(&intent.link.path, &actual, &before)?;
            } else {
                ensure!(actual == intent.before_sha256, "save recovery conflict: an external edit was preserved; backups and pending intent retained");
            }
        } else {
            ensure!(
                save.revision
                    == intent
                        .before_revision
                        .checked_add(1)
                        .context("revision overflow")?
                    && save.bytes == after,
                "save recovery conflict: database advanced unexpectedly; pending intent retained"
            );
            ensure!(actual == intent.after_sha256, "transfer committed in the Vault but the chosen save changed; backups and pending intent retained");
            let mut link = intent.link;
            link.file_sha256 = intent.after_sha256;
            atomic_write(&self.link_path(save.id), &serde_json::to_vec_pretty(&link)?)?;
        }
        fs::remove_file(self.intent_path())?;
        sync_directory(&self.directory)
    }

    fn backup(&self, name: &str, hash: &str) -> Result<Vec<u8>> {
        ensure!(
            Path::new(name).components().count() == 1 && name.ends_with(".sav"),
            "invalid recovery backup path"
        );
        let bytes = read_regular(&self.backups.join(name), 128 * 1024)?;
        ensure!(sha256(&bytes) == hash, "recovery backup hash mismatch");
        Ok(bytes)
    }

    /// Explicit conflict resolution always follows the committed database state;
    /// choosing external bytes here could strand a withdrawn individual outside
    /// both active collections. Preserve those bytes in a separate backup instead.
    pub fn resolve(
        &self,
        store: &ManagedStore,
        id: i64,
        expected_revision: i64,
        expected_file_sha256: &str,
    ) -> Result<()> {
        ensure!(
            self.pending(),
            "there is no pending save transfer to recover"
        );
        let intent: Intent =
            serde_json::from_slice(&read_regular(&self.intent_path(), 64 * 1024)?)?;
        ensure!(
            intent.link.save_id == id,
            "pending transfer belongs to a different save"
        );
        let save = store.save(id)?;
        ensure!(
            save.revision == expected_revision
                && save.profile.fingerprint() == intent.link.profile_sha256,
            "recovery selection changed; refresh first"
        );
        let before = self.backup(&intent.before_backup, &intent.before_sha256)?;
        let after = self.backup(&intent.after_backup, &intent.after_sha256)?;
        ensure!(
            (save.revision == intent.before_revision && save.bytes == before)
                || (Some(save.revision) == intent.before_revision.checked_add(1)
                    && save.bytes == after),
            "database state does not match the pending transfer"
        );
        let external = read_regular(&intent.link.path, 128 * 1024)?;
        ensure!(
            sha256(&external) == expected_file_sha256,
            "chosen save changed since recovery preview; refresh and review again"
        );
        let backup = self.backups.join(format!(
            "save-{id}-conflicting-external-{}.sav",
            Uuid::new_v4()
        ));
        write_new(&backup, &external)?;
        sync_directory(&self.backups)?;
        replace_save(&intent.link.path, expected_file_sha256, &save.bytes)?;
        self.recover(store)
    }
}

/// Source reads are bounded and reject symbolic/hard-link aliases. Parent
/// symlinks (such as NOXgames' directory alias) are canonicalized on Choose save.
pub fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.is_symlink(),
        "save path must be a regular file, not a symbolic link"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.nlink() == 1,
            "hard-linked save files are unsupported; choose a separate regular file"
        );
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "save or metadata exceeds supported size"
    );
    Ok(bytes)
}

fn owned_directory(path: &Path) -> Result<PathBuf> {
    fs::create_dir_all(path)?;
    ensure!(
        !path.is_symlink() && fs::symlink_metadata(path)?.is_dir(),
        "save-link directory must be owned regular storage"
    );
    Ok(path.canonicalize()?)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    ensure!(
        read_regular(path, bytes.len() as u64)? == bytes,
        "save backup verification failed"
    );
    Ok(())
}

/// Stage on the same filesystem as the destination, preserve permissions, then
/// rename. No partially written save image is exposed at the selected filename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("missing output parent")?;
    let temporary = parent.join(format!(".vault-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        write_new(&temporary, bytes)?;
        if path.exists() {
            ensure!(
                !path.is_symlink() && fs::symlink_metadata(path)?.is_file(),
                "refused nonregular replacement target"
            );
            fs::set_permissions(&temporary, fs::metadata(path)?.permissions())?;
        }
        fs::rename(&temporary, path)?;
        sync_directory(parent)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_save(path: &Path, expected: &str, bytes: &[u8]) -> Result<()> {
    ensure!(
        path.canonicalize()? == path,
        "chosen save path changed through an alias"
    );
    ensure!(
        sha256(&read_regular(path, 128 * 1024)?) == expected,
        "chosen save changed before replacement; external bytes preserved"
    );
    atomic_write(path, bytes)
}

// POSIX directory fsync makes rename/intent persistence explicit. Windows file
// durability requires platform runtime validation before claiming the same proof.
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
