//! SQLite storage for opaque managed saves and vaulted Pokémon records.
//!
//! This module deliberately does not decode or rewrite either binary format.
//! Every mutation uses one immediate transaction, so a failed save or Vault
//! change leaves both sides at their previous revision.

use anyhow::{bail, ensure, Context, Result};
use rusqlite::{
    params, Connection, DatabaseName, OptionalExtension, Transaction, TransactionBehavior,
};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;

use crate::{profile::GameProfile, sha256};

/// A checked snapshot of a managed save. Pass this snapshot to `commit`.
#[derive(Clone, Debug)]
pub struct ManagedSave {
    pub id: i64,
    pub profile: GameProfile,
    pub bytes: Vec<u8>,
    pub revision: i64,
}

/// A Vault entry, including the exact source bytes and interpretation context.
#[derive(Clone, Debug, PartialEq)]
pub struct VaultRecord {
    pub id: i64,
    /// Stable identity retained across Vault withdrawal and undo.
    pub uuid: Uuid,
    pub raw: Vec<u8>,
    pub normalized: Value,
    pub source_profile: GameProfile,
    pub source_definition: Value,
}

/// A Vault entry to insert as part of a managed save commit.
#[derive(Clone, Debug)]
pub struct NewVaultRecord {
    pub raw: Vec<u8>,
    pub normalized: Value,
    pub source_profile: GameProfile,
    pub source_definition: Value,
    /// Caller-computed profile, original-save snapshot, slot and raw identity.
    /// Copies cannot replay it. A verified move may retain its active UUID or
    /// restore that UUID after a proven withdrawal into this same managed save.
    pub import_key: Option<String>,
}

/// A ROM's caller-validated identity and optional immutable definition snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct RomLibraryEntry {
    pub hash: String,
    pub source_path: String,
    pub identity: Value,
    pub definition_hash: Option<String>,
}

/// One persisted operation, ordered newest first by `history`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OperationView {
    pub id: i64,
    pub save_id: i64,
    pub before_revision: i64,
    pub after_revision: i64,
    pub kind: String,
    pub undone: bool,
}

/// One SQLite database holding immutable imports and versioned managed state.
pub struct ManagedStore {
    conn: Connection,
    save_writer: Option<SaveWriter>,
}

/// Optional filesystem participant called inside the SQLite transaction after
/// validation, before commit. The desktop supplies a recoverable journal writer;
/// ordinary CLI/library callers remain independent of external files.
pub type SaveWriter = Box<dyn Fn(i64, i64, &[u8], &[u8]) -> Result<()> + Send>;

/// Opaque authority for one active emulator session on a managed save.
#[derive(Clone, Debug)]
pub struct SessionLease {
    pub save_id: i64,
    token: Uuid,
}

impl SessionLease {
    /// Return the token for a recovery record held by the session owner.
    pub fn token(&self) -> Uuid {
        self.token
    }
}

#[derive(Clone)]
struct StoredRecord {
    id: i64,
    uuid: Uuid,
    raw: Vec<u8>,
    raw_sha: String,
    normalized: Vec<u8>,
    normalized_sha: String,
    profile: Vec<u8>,
    profile_sha: String,
    definition: Vec<u8>,
    definition_sha: String,
}

impl StoredRecord {
    fn new(id: i64, record: &NewVaultRecord) -> Result<Self> {
        let normalized = serde_json::to_vec(&record.normalized)?;
        let profile = serde_json::to_vec(&record.source_profile)?;
        let definition = serde_json::to_vec(&record.source_definition)?;
        Ok(Self {
            id,
            uuid: Uuid::new_v4(),
            raw: record.raw.clone(),
            raw_sha: sha256(&record.raw),
            normalized_sha: sha256(&normalized),
            normalized,
            profile_sha: sha256(&profile),
            profile,
            definition_sha: sha256(&definition),
            definition,
        })
    }

    fn verify(&self) -> Result<()> {
        ensure!(
            sha256(&self.raw) == self.raw_sha,
            "Vault {} raw hash mismatch",
            self.id
        );
        ensure!(
            sha256(&self.normalized) == self.normalized_sha,
            "Vault {} normalized hash mismatch",
            self.id
        );
        ensure!(
            sha256(&self.profile) == self.profile_sha,
            "Vault {} profile hash mismatch",
            self.id
        );
        ensure!(
            sha256(&self.definition) == self.definition_sha,
            "Vault {} definition hash mismatch",
            self.id
        );
        Ok(())
    }

    fn public(self) -> Result<VaultRecord> {
        self.verify()?;
        Ok(VaultRecord {
            id: self.id,
            uuid: self.uuid,
            raw: self.raw,
            normalized: serde_json::from_slice(&self.normalized)?,
            source_profile: serde_json::from_slice(&self.profile)?,
            source_definition: serde_json::from_slice(&self.definition)?,
        })
    }
}

fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredRecord> {
    Ok(StoredRecord {
        id: row.get(0)?,
        uuid: Uuid::parse_str(&row.get::<_, String>(1)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?,
        raw: row.get(2)?,
        raw_sha: row.get(3)?,
        normalized: row.get(4)?,
        normalized_sha: row.get(5)?,
        profile: row.get(6)?,
        profile_sha: row.get(7)?,
        definition: row.get(8)?,
        definition_sha: row.get(9)?,
    })
}

const RECORD_COLUMNS: &str = "id, uuid, raw, raw_sha, normalized, normalized_sha, source_profile, source_profile_sha, source_definition, source_definition_sha";

fn load_record(conn: &Connection, id: i64) -> Result<StoredRecord> {
    let sql = format!("SELECT {RECORD_COLUMNS} FROM vault WHERE id = ?1");
    let record = conn
        .query_row(&sql, [id], record_from_row)
        .optional()?
        .with_context(|| format!("Vault record {id} does not exist"))?;
    record.verify()?;
    Ok(record)
}

fn insert_record(tx: &Transaction<'_>, record: &StoredRecord) -> Result<()> {
    tx.execute(
        "INSERT INTO vault (id, uuid, raw, raw_sha, normalized, normalized_sha, source_profile, source_profile_sha, source_definition, source_definition_sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![record.id, record.uuid.to_string(), record.raw, record.raw_sha, record.normalized, record.normalized_sha, record.profile, record.profile_sha, record.definition, record.definition_sha],
    )?;
    Ok(())
}

fn log_event(
    tx: &Transaction<'_>,
    operation_id: i64,
    action: &str,
    record: &StoredRecord,
) -> Result<()> {
    tx.execute(
        "INSERT INTO vault_events (operation_id, vault_id, uuid, action, raw, raw_sha, normalized, normalized_sha, source_profile, source_profile_sha, source_definition, source_definition_sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![operation_id, record.id, record.uuid.to_string(), action, record.raw, record.raw_sha, record.normalized, record.normalized_sha, record.profile, record.profile_sha, record.definition, record.definition_sha],
    )?;
    Ok(())
}

fn has_lease(tx: &Transaction<'_>, save_id: i64) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_leases WHERE save_id = ?1)",
        [save_id],
        |r| r.get(0),
    )?)
}

fn valid_sha256(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn rom_metadata_sha(path: &str, identity: &[u8], definition: Option<&str>) -> Result<String> {
    Ok(sha256(&serde_json::to_vec(&(path, identity, definition))?))
}

fn checked_rom(
    conn: &Connection,
    hash: String,
    source_path: String,
    identity_bytes: Vec<u8>,
    metadata_sha: String,
    definition_hash: Option<String>,
) -> Result<RomLibraryEntry> {
    ensure!(valid_sha256(&hash), "ROM library hash is malformed");
    ensure!(
        rom_metadata_sha(&source_path, &identity_bytes, definition_hash.as_deref())?
            == metadata_sha,
        "ROM {hash} metadata hash mismatch"
    );
    let identity: Value = serde_json::from_slice(&identity_bytes)?;
    ensure!(
        identity.get("sha256").and_then(Value::as_str) == Some(hash.as_str()),
        "ROM {hash} identity does not match its hash"
    );
    if let Some(definition_hash) = &definition_hash {
        let bytes: Vec<u8> = conn
            .query_row(
                "SELECT bytes FROM definition_snapshots WHERE hash = ?1",
                [definition_hash],
                |row| row.get(0),
            )
            .optional()?
            .with_context(|| format!("ROM {hash} definition snapshot is missing"))?;
        ensure!(
            sha256(&bytes) == *definition_hash,
            "ROM {hash} definition snapshot hash mismatch"
        );
    }
    Ok(RomLibraryEntry {
        hash,
        source_path,
        identity,
        definition_hash,
    })
}

impl ManagedStore {
    /// Open or create a store. SQLite enforces foreign keys and full synchronous commits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= 3,
            "store schema version {version} is newer than supported version 3"
        );
        if version == 0 {
            let has_old_tables: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'saves')",
                [], |r| r.get(0))?;
            ensure!(
                !has_old_tables,
                "unversioned store requires an explicit migration"
            );
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS saves (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                profile BLOB NOT NULL, profile_sha TEXT NOT NULL,
                bytes BLOB NOT NULL, bytes_sha TEXT NOT NULL,
                revision INTEGER NOT NULL CHECK (revision >= 0)
            );
            CREATE TABLE IF NOT EXISTS original_imports (
                save_id INTEGER PRIMARY KEY REFERENCES saves(id),
                profile BLOB NOT NULL, profile_sha TEXT NOT NULL,
                bytes BLOB NOT NULL, bytes_sha TEXT NOT NULL
            );
            CREATE TRIGGER IF NOT EXISTS preserve_original_update BEFORE UPDATE ON original_imports
                BEGIN SELECT RAISE(ABORT, 'original import is immutable'); END;
            CREATE TRIGGER IF NOT EXISTS preserve_original_delete BEFORE DELETE ON original_imports
                BEGIN SELECT RAISE(ABORT, 'original import is immutable'); END;
            CREATE TABLE IF NOT EXISTS save_revisions (
                save_id INTEGER NOT NULL REFERENCES saves(id), revision INTEGER NOT NULL,
                bytes BLOB NOT NULL, bytes_sha TEXT NOT NULL,
                PRIMARY KEY (save_id, revision)
            );
            CREATE TRIGGER IF NOT EXISTS preserve_revision_update BEFORE UPDATE ON save_revisions
                BEGIN SELECT RAISE(ABORT, 'save revision is immutable'); END;
            CREATE TRIGGER IF NOT EXISTS preserve_revision_delete BEFORE DELETE ON save_revisions
                BEGIN SELECT RAISE(ABORT, 'save revision is immutable'); END;
            CREATE TABLE IF NOT EXISTS vault (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uuid TEXT NOT NULL UNIQUE,
                raw BLOB NOT NULL, raw_sha TEXT NOT NULL,
                normalized BLOB NOT NULL, normalized_sha TEXT NOT NULL,
                source_profile BLOB NOT NULL, source_profile_sha TEXT NOT NULL,
                source_definition BLOB NOT NULL, source_definition_sha TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS operations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                save_id INTEGER NOT NULL REFERENCES saves(id),
                before_revision INTEGER NOT NULL, after_revision INTEGER NOT NULL,
                kind TEXT NOT NULL, undone INTEGER NOT NULL DEFAULT 0 CHECK (undone IN (0, 1)),
                UNIQUE (save_id, after_revision)
            );
            CREATE TABLE IF NOT EXISTS vault_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_id INTEGER NOT NULL REFERENCES operations(id),
                vault_id INTEGER NOT NULL, uuid TEXT NOT NULL,
                action TEXT NOT NULL CHECK (action IN ('insert', 'remove')),
                raw BLOB NOT NULL, raw_sha TEXT NOT NULL,
                normalized BLOB NOT NULL, normalized_sha TEXT NOT NULL,
                source_profile BLOB NOT NULL, source_profile_sha TEXT NOT NULL,
                source_definition BLOB NOT NULL, source_definition_sha TEXT NOT NULL
            );
            CREATE TRIGGER IF NOT EXISTS preserve_event_update BEFORE UPDATE ON vault_events
                BEGIN SELECT RAISE(ABORT, 'Vault event is immutable'); END;
            CREATE TRIGGER IF NOT EXISTS preserve_event_delete BEFORE DELETE ON vault_events
                BEGIN SELECT RAISE(ABORT, 'Vault event is immutable'); END;
            CREATE TABLE IF NOT EXISTS session_leases (
                save_id INTEGER PRIMARY KEY REFERENCES saves(id),
                token TEXT NOT NULL UNIQUE,
                acquired_revision INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS import_receipts (
                import_key TEXT PRIMARY KEY,
                vault_uuid TEXT NOT NULL,
                operation_id INTEGER NOT NULL REFERENCES operations(id)
            );
            CREATE TRIGGER IF NOT EXISTS preserve_receipt_update BEFORE UPDATE ON import_receipts
                BEGIN SELECT RAISE(ABORT, 'import receipt is immutable'); END;
            CREATE TRIGGER IF NOT EXISTS preserve_receipt_delete BEFORE DELETE ON import_receipts
                BEGIN SELECT RAISE(ABORT, 'import receipt is immutable'); END;
            CREATE TABLE IF NOT EXISTS definition_snapshots (
                hash TEXT PRIMARY KEY,
                bytes BLOB NOT NULL
            );
            CREATE TRIGGER IF NOT EXISTS preserve_definition_update BEFORE UPDATE ON definition_snapshots
                BEGIN SELECT RAISE(ABORT, 'definition snapshot is immutable'); END;
            CREATE TRIGGER IF NOT EXISTS preserve_definition_delete BEFORE DELETE ON definition_snapshots
                BEGIN SELECT RAISE(ABORT, 'definition snapshot is immutable'); END;",
        )?;
        // Schema 2 only adds the ROM library. Existing schema-1 rows and their
        // immutable triggers are untouched; the version changes in this same
        // immediate transaction only after the new table exists.
        if version < 2 {
            tx.execute_batch(
                "CREATE TABLE rom_library (
                    hash TEXT PRIMARY KEY,
                    source_path TEXT NOT NULL,
                    identity BLOB NOT NULL,
                    metadata_sha TEXT NOT NULL,
                    definition_hash TEXT REFERENCES definition_snapshots(hash)
                );",
            )?;
            tx.pragma_update(None, "user_version", 2)?;
        }
        // Retain records a deposit that removes a source copy while reusing its
        // proven existing Vault identity. Keep historical event IDs and bytes;
        // widening a CHECK constraint requires rebuilding this one table.
        if version < 3 {
            tx.execute_batch(
                "CREATE TABLE vault_events_v3 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_id INTEGER NOT NULL REFERENCES operations(id),
                vault_id INTEGER NOT NULL, uuid TEXT NOT NULL,
                action TEXT NOT NULL CHECK (action IN ('insert','remove','retain')),
                raw BLOB NOT NULL, raw_sha TEXT NOT NULL,
                normalized BLOB NOT NULL, normalized_sha TEXT NOT NULL,
                source_profile BLOB NOT NULL, source_profile_sha TEXT NOT NULL,
                source_definition BLOB NOT NULL, source_definition_sha TEXT NOT NULL
            );
            INSERT INTO vault_events_v3 SELECT * FROM vault_events;
            DROP TABLE vault_events;
            ALTER TABLE vault_events_v3 RENAME TO vault_events;
            CREATE TRIGGER preserve_event_update BEFORE UPDATE ON vault_events
                BEGIN SELECT RAISE(ABORT, 'Vault event is immutable'); END;
            CREATE TRIGGER preserve_event_delete BEFORE DELETE ON vault_events
                BEGIN SELECT RAISE(ABORT, 'Vault event is immutable'); END;",
            )?;
            tx.pragma_update(None, "user_version", 3)?;
        }
        tx.commit()?;
        Ok(Self {
            conn,
            save_writer: None,
        })
    }

    /// Install for one operation only. A failed file publish aborts all save and
    /// Vault SQL changes; its durable journal resolves crashes around commit.
    pub fn set_save_writer(&mut self, writer: Option<SaveWriter>) {
        self.save_writer = writer;
    }

    /// Forget a library entry only. Historical definitions, saves and Vault
    /// records deliberately have independent ownership and are never cascaded.
    pub fn remove_rom(&mut self, hash: &str) -> Result<()> {
        self.rom(hash)?;
        self.conn
            .execute("DELETE FROM rom_library WHERE hash = ?1", [hash])?;
        Ok(())
    }

    /// Copy caller-owned bytes into an immutable import and revision-zero snapshot.
    pub fn import_save(&mut self, profile: &GameProfile, bytes: &[u8]) -> Result<i64> {
        let profile = serde_json::to_vec(profile)?;
        let profile_sha = sha256(&profile);
        let bytes_sha = sha256(bytes);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO saves (profile, profile_sha, bytes, bytes_sha, revision) VALUES (?1, ?2, ?3, ?4, 0)", params![profile, profile_sha, bytes, bytes_sha])?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO original_imports (save_id, profile, profile_sha, bytes, bytes_sha) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, profile, profile_sha, bytes, bytes_sha])?;
        tx.execute("INSERT INTO save_revisions (save_id, revision, bytes, bytes_sha) VALUES (?1, 0, ?2, ?3)", params![id, bytes, bytes_sha])?;
        tx.commit()?;
        Ok(id)
    }

    /// Read a checked managed save snapshot for optimistic commit.
    pub fn save(&self, id: i64) -> Result<ManagedSave> {
        let (profile, profile_sha, bytes, bytes_sha, revision): (
            Vec<u8>,
            String,
            Vec<u8>,
            String,
            i64,
        ) = self
            .conn
            .query_row(
                "SELECT profile, profile_sha, bytes, bytes_sha, revision FROM saves WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .with_context(|| format!("managed save {id} does not exist"))?;
        ensure!(
            sha256(&profile) == profile_sha,
            "save {id} profile hash mismatch"
        );
        ensure!(
            sha256(&bytes) == bytes_sha,
            "save {id} content hash mismatch"
        );
        let (revision_bytes, revision_sha): (Vec<u8>, String) = self.conn.query_row(
            "SELECT bytes, bytes_sha FROM save_revisions WHERE save_id = ?1 AND revision = ?2",
            params![id, revision],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            sha256(&revision_bytes) == revision_sha && revision_bytes == bytes,
            "save {id} revision snapshot mismatch"
        );
        let (original_profile, original_profile_sha, original, original_sha):
            (Vec<u8>, String, Vec<u8>, String) = self.conn.query_row(
            "SELECT profile, profile_sha, bytes, bytes_sha FROM original_imports WHERE save_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        ensure!(
            sha256(&original_profile) == original_profile_sha && original_profile == profile,
            "save {id} original profile mismatch"
        );
        ensure!(
            sha256(&original) == original_sha,
            "save {id} original hash mismatch"
        );
        Ok(ManagedSave {
            id,
            profile: serde_json::from_slice(&profile)?,
            bytes,
            revision,
        })
    }

    /// List checked Vault records in stable ID order.
    pub fn list_vault(&self) -> Result<Vec<VaultRecord>> {
        let sql = format!("SELECT {RECORD_COLUMNS} FROM vault ORDER BY id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], record_from_row)?;
        rows.map(|row| row?.public()).collect()
    }

    /// Read one checked Vault record by ID.
    pub fn vault(&self, id: i64) -> Result<VaultRecord> {
        load_record(&self.conn, id)?.public()
    }

    /// Stable original-save identity, independent of how many managed copies or
    /// revisions were created. This detects repeated imports without merging
    /// identical individuals that occupied different source slots.
    pub fn original_save_hash(&self, save_id: i64) -> Result<String> {
        self.save(save_id)?; // Verify the immutable import and profile first.
        Ok(self.conn.query_row(
            "SELECT bytes_sha FROM original_imports WHERE save_id = ?1",
            [save_id],
            |row| row.get(0),
        )?)
    }

    /// Register an immutable JSON definition bundle by its serialized SHA-256.
    /// Re-registering the same bundle returns the existing digest and row.
    pub fn register_definition(&mut self, definition: &Value) -> Result<String> {
        let bytes = serde_json::to_vec(definition)?;
        let hash = sha256(&bytes);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT OR IGNORE INTO definition_snapshots (hash, bytes) VALUES (?1, ?2)",
            params![hash, bytes],
        )?;
        let existing: Vec<u8> = tx.query_row(
            "SELECT bytes FROM definition_snapshots WHERE hash = ?1",
            [&hash],
            |r| r.get(0),
        )?;
        ensure!(
            existing == bytes,
            "definition digest collision or corrupted snapshot"
        );
        tx.commit()?;
        Ok(hash)
    }

    /// Resolve a registered definition bundle after verifying its digest.
    pub fn definition(&self, hash: &str) -> Result<Value> {
        let bytes: Vec<u8> = self
            .conn
            .query_row(
                "SELECT bytes FROM definition_snapshots WHERE hash = ?1",
                [hash],
                |r| r.get(0),
            )
            .optional()?
            .with_context(|| format!("definition snapshot {hash} does not exist"))?;
        ensure!(
            sha256(&bytes) == hash,
            "definition snapshot {hash} hash mismatch"
        );
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Definitions outlive library visibility, preserving interpretation of old
    /// saves and Vault entries after the user removes a game from the list.
    pub fn definition_hashes(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash FROM definition_snapshots ORDER BY hash")?;
        let rows = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Register a caller-verified ROM. A repeated hash may only move its source
    /// path; changing its identity or definition requires an explicit migration.
    pub fn register_rom(&mut self, entry: &RomLibraryEntry) -> Result<()> {
        ensure!(
            valid_sha256(&entry.hash),
            "ROM hash must be lowercase SHA-256"
        );
        ensure!(
            entry.identity.get("sha256").and_then(Value::as_str) == Some(entry.hash.as_str()),
            "ROM identity SHA-256 does not match hash"
        );
        let identity = serde_json::to_vec(&entry.identity)?;
        let metadata_sha = rom_metadata_sha(
            &entry.source_path,
            &identity,
            entry.definition_hash.as_deref(),
        )?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(hash) = &entry.definition_hash {
            let bytes: Vec<u8> = tx
                .query_row(
                    "SELECT bytes FROM definition_snapshots WHERE hash = ?1",
                    [hash],
                    |row| row.get(0),
                )
                .optional()?
                .with_context(|| format!("definition snapshot {hash} does not exist"))?;
            ensure!(
                sha256(&bytes) == *hash,
                "definition snapshot {hash} hash mismatch"
            );
        }
        let existing: Option<(String, Vec<u8>, String, Option<String>)> = tx
            .query_row(
                "SELECT source_path, identity, metadata_sha, definition_hash FROM rom_library WHERE hash = ?1",
                [&entry.hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        if let Some((old_path, old_identity, old_sha, old_definition)) = existing {
            ensure!(
                rom_metadata_sha(&old_path, &old_identity, old_definition.as_deref())? == old_sha,
                "ROM metadata hash mismatch"
            );
            ensure!(
                old_identity == identity && old_definition == entry.definition_hash,
                "ROM metadata differs from registered identity"
            );
            tx.execute(
                "UPDATE rom_library SET source_path = ?1, metadata_sha = ?2 WHERE hash = ?3",
                params![entry.source_path, metadata_sha, entry.hash],
            )?;
        } else {
            tx.execute("INSERT INTO rom_library (hash, source_path, identity, metadata_sha, definition_hash) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entry.hash, entry.source_path, identity, metadata_sha, entry.definition_hash])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// List registered ROMs after checking identity bytes and definition links.
    pub fn list_roms(&self) -> Result<Vec<RomLibraryEntry>> {
        let mut stmt = self.conn.prepare("SELECT hash, source_path, identity, metadata_sha, definition_hash FROM rom_library ORDER BY hash")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<(String, String, Vec<u8>, String, Option<String>)>>>(
            )?;
        rows.into_iter()
            .map(|(hash, path, identity, sha, definition)| {
                checked_rom(&self.conn, hash, path, identity, sha, definition)
            })
            .collect()
    }

    /// Read one registered ROM after checking its persisted metadata.
    pub fn rom(&self, hash: &str) -> Result<RomLibraryEntry> {
        let (hash, path, identity, sha, definition): (String, String, Vec<u8>, String, Option<String>) = self.conn.query_row(
            "SELECT hash, source_path, identity, metadata_sha, definition_hash FROM rom_library WHERE hash = ?1",
            [hash], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).optional()?.with_context(|| format!("ROM {hash} is not registered"))?;
        checked_rom(&self.conn, hash, path, identity, sha, definition)
    }

    /// List managed save identifiers without interpreting save bytes.
    pub fn list_save_ids(&self) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare("SELECT id FROM saves ORDER BY id")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// List operations newest first, optionally limited to one managed save.
    pub fn history(&self, save_id: Option<i64>) -> Result<Vec<OperationView>> {
        let mut stmt = self.conn.prepare("SELECT id, save_id, before_revision, after_revision, kind, undone FROM operations WHERE (?1 IS NULL OR save_id = ?1) ORDER BY id DESC")?;
        let operations = stmt
            .query_map([save_id], |row| {
                Ok(OperationView {
                    id: row.get(0)?,
                    save_id: row.get(1)?,
                    before_revision: row.get(2)?,
                    after_revision: row.get(3)?,
                    kind: row.get(4)?,
                    undone: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(operations)
    }

    /// Check SQLite structure, foreign keys, and every stored content hash.
    /// This reads all saved revisions and Vault events, including removed items.
    pub fn integrity_check(&self) -> Result<()> {
        let status: String = self
            .conn
            .pragma_query_value(None, "integrity_check", |r| r.get(0))?;
        ensure!(status == "ok", "SQLite integrity check failed: {status}");
        let mut fk = self.conn.prepare("PRAGMA foreign_key_check")?;
        ensure!(
            fk.query([])?.next()?.is_none(),
            "SQLite foreign key check failed"
        );
        drop(fk);
        let mut saves = self.conn.prepare("SELECT id FROM saves ORDER BY id")?;
        let ids: Vec<i64> = saves
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(saves);
        for id in ids {
            self.save(id)?;
        }
        let mut revisions = self
            .conn
            .prepare("SELECT save_id, revision, bytes, bytes_sha FROM save_revisions")?;
        for row in revisions.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })? {
            let (save_id, revision, bytes, hash) = row?;
            ensure!(
                sha256(&bytes) == hash,
                "save {save_id} revision {revision} hash mismatch"
            );
        }
        self.list_vault()?;
        let mut events = self.conn.prepare("SELECT vault_id, uuid, raw, raw_sha, normalized, normalized_sha, source_profile, source_profile_sha, source_definition, source_definition_sha FROM vault_events")?;
        for row in events.query_map([], record_from_row)? {
            row?.verify()?;
        }
        let mut definitions = self
            .conn
            .prepare("SELECT hash, bytes FROM definition_snapshots")?;
        for row in definitions.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })? {
            let (hash, bytes) = row?;
            ensure!(
                sha256(&bytes) == hash,
                "definition snapshot {hash} hash mismatch"
            );
            let _: Value = serde_json::from_slice(&bytes)?;
        }
        self.list_roms()?;
        Ok(())
    }

    /// Export a consistent SQLite backup to a new path. Existing paths are never
    /// overwritten; a failed backup removes only the file created by this call.
    pub fn export_backup(&self, path: impl AsRef<Path>) -> Result<()> {
        self.integrity_check()?;
        let path = path.as_ref();
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        match self
            .conn
            .backup(DatabaseName::Main, path, None)
            .map_err(anyhow::Error::from)
            .and_then(|_| Self::open(path)?.integrity_check())
        {
            Ok(()) => Ok(()),
            Err(error) => {
                std::fs::remove_file(path).context("failed to remove incomplete backup")?;
                Err(error)
            }
        }
    }

    /// Acquire exclusive logical ownership of one save at an expected revision.
    /// A crash leaves the lease in the database and therefore fails closed.
    pub fn acquire_session(
        &mut self,
        save_id: i64,
        expected_revision: i64,
    ) -> Result<SessionLease> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: i64 = tx
            .query_row("SELECT revision FROM saves WHERE id = ?1", [save_id], |r| {
                r.get(0)
            })
            .optional()?
            .with_context(|| format!("managed save {save_id} does not exist"))?;
        ensure!(revision == expected_revision, "stale session revision");
        let token = Uuid::new_v4();
        tx.execute(
            "INSERT INTO session_leases (save_id, token, acquired_revision) VALUES (?1, ?2, ?3)",
            params![save_id, token.to_string(), revision],
        )?;
        tx.commit()?;
        Ok(SessionLease { save_id, token })
    }

    /// Release a session after its emulator has stopped and any save was committed.
    pub fn release_session(&mut self, lease: &SessionLease) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "DELETE FROM session_leases WHERE save_id = ?1 AND token = ?2",
            params![lease.save_id, lease.token.to_string()],
        )?;
        ensure!(
            changed == 1,
            "session lease is absent or token does not match"
        );
        tx.commit()?;
        Ok(())
    }

    /// Recover a stranded lease only after the caller verifies that its emulator
    /// has stopped. The original token is required; no age-based expiry exists.
    pub fn recover_session(&mut self, save_id: i64, token: Uuid) -> Result<()> {
        self.release_session(&SessionLease { save_id, token })
    }

    /// Commit only emulator-produced save bytes while holding the exact lease.
    /// This API cannot mutate Vault records; transfers use `commit` after release.
    pub fn commit_session(
        &mut self,
        snapshot: &ManagedSave,
        new_bytes: &[u8],
        lease: &SessionLease,
    ) -> Result<i64> {
        ensure!(
            lease.save_id == snapshot.id,
            "session lease belongs to another save"
        );
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (token, acquired_revision): (String, i64) = tx
            .query_row(
                "SELECT token, acquired_revision FROM session_leases WHERE save_id = ?1",
                [snapshot.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .context("no active session lease")?;
        ensure!(
            token == lease.token.to_string() && acquired_revision == snapshot.revision,
            "session lease token or revision mismatch"
        );
        let (profile, profile_sha, bytes, bytes_sha, revision): (
            Vec<u8>,
            String,
            Vec<u8>,
            String,
            i64,
        ) = tx.query_row(
            "SELECT profile, profile_sha, bytes, bytes_sha, revision FROM saves WHERE id = ?1",
            [snapshot.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        ensure!(
            sha256(&profile) == profile_sha && sha256(&bytes) == bytes_sha,
            "managed save content corruption"
        );
        ensure!(
            revision == snapshot.revision
                && bytes == snapshot.bytes
                && profile == serde_json::to_vec(&snapshot.profile)?,
            "stale managed save snapshot"
        );
        let next = revision.checked_add(1).context("save revision overflow")?;
        let new_sha = sha256(new_bytes);
        tx.execute(
            "UPDATE saves SET bytes = ?1, bytes_sha = ?2, revision = ?3 WHERE id = ?4",
            params![new_bytes, new_sha, next, snapshot.id],
        )?;
        tx.execute("INSERT INTO save_revisions (save_id, revision, bytes, bytes_sha) VALUES (?1, ?2, ?3, ?4)",
            params![snapshot.id, next, new_bytes, new_sha])?;
        tx.execute("INSERT INTO operations (save_id, before_revision, after_revision, kind) VALUES (?1, ?2, ?3, 'session')",
            params![snapshot.id, revision, next])?;
        // A session may continue from the committed revision without releasing
        // ownership. A fresh snapshot is required for each subsequent commit.
        tx.execute(
            "UPDATE session_leases SET acquired_revision = ?1 WHERE save_id = ?2",
            params![next, snapshot.id],
        )?;
        if let Some(writer) = &self.save_writer {
            writer(snapshot.id, snapshot.revision, &snapshot.bytes, new_bytes)?;
        }
        tx.commit()?;
        Ok(next)
    }

    /// Atomically advance a save and insert/remove the specified Vault entries.
    /// `snapshot` must still match the current revision, profile and bytes.
    pub fn commit(
        &mut self,
        snapshot: &ManagedSave,
        new_bytes: &[u8],
        kind: &str,
        additions: &[NewVaultRecord],
        removals: &[i64],
    ) -> Result<i64> {
        ensure!(
            !kind.is_empty() && kind != "undo" && kind != "session",
            "invalid commit kind"
        );
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            !has_lease(&tx, snapshot.id)?,
            "managed save has an active session lease"
        );
        let (profile, profile_sha, bytes, bytes_sha, revision): (
            Vec<u8>,
            String,
            Vec<u8>,
            String,
            i64,
        ) = tx
            .query_row(
                "SELECT profile, profile_sha, bytes, bytes_sha, revision FROM saves WHERE id = ?1",
                [snapshot.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .with_context(|| format!("managed save {} does not exist", snapshot.id))?;
        ensure!(
            sha256(&profile) == profile_sha && sha256(&bytes) == bytes_sha,
            "managed save content corruption"
        );
        ensure!(
            revision == snapshot.revision
                && bytes == snapshot.bytes
                && profile == serde_json::to_vec(&snapshot.profile)?,
            "stale managed save snapshot"
        );
        let next = revision.checked_add(1).context("save revision overflow")?;
        let new_sha = sha256(new_bytes);
        tx.execute(
            "UPDATE saves SET bytes = ?1, bytes_sha = ?2, revision = ?3 WHERE id = ?4",
            params![new_bytes, new_sha, next, snapshot.id],
        )?;
        tx.execute("INSERT INTO save_revisions (save_id, revision, bytes, bytes_sha) VALUES (?1, ?2, ?3, ?4)",
            params![snapshot.id, next, new_bytes, new_sha])?;
        tx.execute("INSERT INTO operations (save_id, before_revision, after_revision, kind) VALUES (?1, ?2, ?3, ?4)",
            params![snapshot.id, revision, next, kind])?;
        let operation_id = tx.last_insert_rowid();
        // Load before removing so hash failures and missing IDs abort the whole transaction.
        for &id in removals {
            let record = load_record(&tx, id)?;
            tx.execute("DELETE FROM vault WHERE id = ?1", [id])?;
            log_event(&tx, operation_id, "remove", &record)?;
        }
        for addition in additions {
            let mut record = StoredRecord::new(0, addition)?;
            if let Some(key) = &addition.import_key {
                ensure!(!key.is_empty(), "empty Vault import key");
                // Receipts survive removal and undo. Stable source snapshot and
                // slot identity distinguish legitimate identical individuals.
                // Explain a replay before SQLite reports an implementation-level
                // UNIQUE error; the surrounding transaction still rolls back all.
                let existing: Option<String> = tx
                    .query_row(
                        "SELECT vault_uuid FROM import_receipts WHERE import_key=?1",
                        [key],
                        |row| row.get(0),
                    )
                    .optional()?;
                if let Some(uuid) = existing {
                    ensure!(kind == "managed-deposit", "This Pokemon was already copied from that source slot; use Deposit to move it rather than copying it again.");
                    let present = tx
                        .query_row(
                            &format!("SELECT {RECORD_COLUMNS} FROM vault WHERE uuid=?1"),
                            [&uuid],
                            record_from_row,
                        )
                        .optional()?;
                    if let Some(present) = present {
                        present.verify()?;
                        ensure!(
                            present.raw == record.raw
                                && present.profile == record.profile
                                && present.definition == record.definition,
                            "existing source receipt does not match this Pokemon/profile"
                        );
                        // The exact import receipt proves identity; raw equality
                        // alone never merges two independently identical Pokemon.
                        log_event(&tx, operation_id, "retain", &present)?;
                        continue;
                    }
                    // A normal deposit/withdraw/deposit cycle may return to its
                    // original source slot. Reuse its UUID only if its last event
                    // proves a withdrawal into this very managed save.
                    let returned = tx.query_row("SELECT e.vault_id,e.uuid,e.raw,e.raw_sha,e.normalized,e.normalized_sha,e.source_profile,e.source_profile_sha,e.source_definition,e.source_definition_sha
                        FROM vault_events e JOIN operations o ON o.id=e.operation_id
                        WHERE e.uuid=?1 AND e.id=(SELECT MAX(id) FROM vault_events WHERE uuid=?1)
                        AND e.action='remove' AND o.kind='withdraw' AND o.save_id=?2 AND o.undone=0",
                        params![uuid,snapshot.id], record_from_row).optional()?.context("source was imported previously but has no active or verified returned Vault record")?;
                    returned.verify()?;
                    ensure!(
                        returned.raw == record.raw
                            && returned.profile == record.profile
                            && returned.definition == record.definition,
                        "returned Pokemon no longer matches its preserved source record"
                    );
                    insert_record(&tx, &returned)?;
                    log_event(&tx, operation_id, "insert", &returned)?;
                    continue;
                }
                tx.execute("INSERT INTO import_receipts (import_key, vault_uuid, operation_id) VALUES (?1, ?2, ?3)",
                    params![key, record.uuid.to_string(), operation_id])?;
            }
            tx.execute("INSERT INTO vault (uuid, raw, raw_sha, normalized, normalized_sha, source_profile, source_profile_sha, source_definition, source_definition_sha) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![record.uuid.to_string(), record.raw, record.raw_sha, record.normalized, record.normalized_sha, record.profile, record.profile_sha, record.definition, record.definition_sha])?;
            record.id = tx.last_insert_rowid();
            log_event(&tx, operation_id, "insert", &record)?;
        }
        if let Some(writer) = &self.save_writer {
            writer(snapshot.id, snapshot.revision, &snapshot.bytes, new_bytes)?;
        }
        tx.commit()?;
        Ok(next)
    }

    /// Reverse the latest transfer for this save only when every touched Vault
    /// record still has that transfer as its last event. The undo is itself an
    /// atomic, monotonic save revision and logs its inverse Vault events.
    pub fn undo(&mut self, save_id: i64) -> Result<i64> {
        self.undo_inner(save_id, None)
    }

    /// Undo only when the managed save still has the caller's expected revision.
    /// The revision check and rollback share one immediate transaction.
    pub fn undo_at_revision(&mut self, save_id: i64, expected_revision: i64) -> Result<i64> {
        self.undo_inner(save_id, Some(expected_revision))
    }

    fn undo_inner(&mut self, save_id: i64, expected_revision: Option<i64>) -> Result<i64> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            !has_lease(&tx, save_id)?,
            "managed save has an active session lease"
        );
        let (current, current_sha, revision): (Vec<u8>, String, i64) = tx
            .query_row(
                "SELECT bytes, bytes_sha, revision FROM saves WHERE id = ?1",
                [save_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .with_context(|| format!("managed save {save_id} does not exist"))?;
        if let Some(expected) = expected_revision {
            ensure!(revision == expected, "stale managed save revision");
        }
        ensure!(
            sha256(&current) == current_sha,
            "managed save content corruption"
        );
        let (operation_id, before_revision, kind): (i64, i64, String) = tx.query_row(
            "SELECT id, before_revision, kind FROM operations WHERE save_id = ?1 AND after_revision = ?2 AND undone = 0",
            params![save_id, revision], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?.context("no latest transfer to undo")?;
        ensure!(
            kind != "undo" && kind != "session",
            "latest operation is not an undoable transfer"
        );
        let (prior, prior_sha): (Vec<u8>, String) = tx.query_row(
            "SELECT bytes, bytes_sha FROM save_revisions WHERE save_id = ?1 AND revision = ?2",
            params![save_id, before_revision],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            sha256(&prior) == prior_sha,
            "prior save revision hash mismatch"
        );

        // Event IDs are global. Any later transfer or undo touching an entry
        // changes its latest event, even when the entry is currently absent.
        let mut stmt = tx.prepare("SELECT id, vault_id, uuid, action, raw, raw_sha, normalized, normalized_sha, source_profile, source_profile_sha, source_definition, source_definition_sha FROM vault_events WHERE operation_id = ?1 ORDER BY id DESC")?;
        let events: Vec<(i64, String, StoredRecord)> = stmt
            .query_map([operation_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(3)?,
                    StoredRecord {
                        id: r.get(1)?,
                        uuid: Uuid::parse_str(&r.get::<_, String>(2)?).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                2,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?,
                        raw: r.get(4)?,
                        raw_sha: r.get(5)?,
                        normalized: r.get(6)?,
                        normalized_sha: r.get(7)?,
                        profile: r.get(8)?,
                        profile_sha: r.get(9)?,
                        definition: r.get(10)?,
                        definition_sha: r.get(11)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        for (event_id, action, record) in &events {
            record.verify()?;
            let latest: i64 = tx.query_row(
                "SELECT MAX(id) FROM vault_events WHERE vault_id = ?1",
                [record.id],
                |r| r.get(0),
            )?;
            ensure!(
                latest == *event_id,
                "Vault record {} was touched later",
                record.id
            );
            match action.as_str() {
                "insert" | "retain" => {
                    let present = load_record(&tx, record.id)?;
                    ensure!(
                        present.uuid == record.uuid
                            && present.raw_sha == record.raw_sha
                            && present.normalized_sha == record.normalized_sha
                            && present.profile_sha == record.profile_sha
                            && present.definition_sha == record.definition_sha,
                        "Vault record {} changed",
                        record.id
                    );
                }
                "remove" => {
                    let exists: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM vault WHERE id = ?1)",
                        [record.id],
                        |r| r.get(0),
                    )?;
                    ensure!(!exists, "Vault record {} was restored", record.id);
                }
                _ => bail!("invalid Vault event action"),
            }
        }
        let next = revision.checked_add(1).context("save revision overflow")?;
        tx.execute(
            "UPDATE saves SET bytes = ?1, bytes_sha = ?2, revision = ?3 WHERE id = ?4",
            params![prior, prior_sha, next, save_id],
        )?;
        tx.execute("INSERT INTO save_revisions (save_id, revision, bytes, bytes_sha) VALUES (?1, ?2, ?3, ?4)",
            params![save_id, next, prior, prior_sha])?;
        tx.execute("INSERT INTO operations (save_id, before_revision, after_revision, kind) VALUES (?1, ?2, ?3, 'undo')",
            params![save_id, revision, next])?;
        let undo_id = tx.last_insert_rowid();
        for (_, action, record) in events {
            if action == "retain" {
                // Undo restores the source slot but keeps a pre-existing Vault
                // copy; this was not a new insertion to delete.
                log_event(&tx, undo_id, "retain", &record)?;
            } else if action == "insert" {
                tx.execute("DELETE FROM vault WHERE id = ?1", [record.id])?;
                log_event(&tx, undo_id, "remove", &record)?;
            } else {
                insert_record(&tx, &record)?;
                log_event(&tx, undo_id, "insert", &record)?;
            }
        }
        tx.execute(
            "UPDATE operations SET undone = 1 WHERE id = ?1",
            [operation_id],
        )?;
        if let Some(writer) = &self.save_writer {
            writer(save_id, revision, &current, &prior)?;
        }
        tx.commit()?;
        Ok(next)
    }
}
