//! Local app-lock authentication (fully offline).
//!
//! NEXORA can require a username + password before the library opens. This is a
//! **local** lock: credentials live only in this machine's SQLite database, the
//! password is stored as an Argon2id PHC hash (never plaintext, never sent over a
//! network), and there is no server involved. The threat model is "someone else
//! sitting at this computer" — it gates access to the app UI. It is not
//! whole-disk encryption: the texture files themselves remain readable on disk by
//! anyone with filesystem access.
//!
//! First run has no users, so the app shows a one-time setup screen to create the
//! first account; afterwards it shows a login screen each launch (the session is
//! held in memory by the desktop shell, so quitting the app logs out).

use crate::{CoreError, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Minimum password length. Deliberately modest — this is a local convenience
/// lock, not a public-facing service — but enough to stop a trivial guess.
pub const MIN_PASSWORD_LEN: usize = 6;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A user account, minus the password hash (never surfaced to the UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub created_at: i64,
    pub last_login: Option<i64>,
}

fn read_user(row: &rusqlite::Row) -> rusqlite::Result<UserInfo> {
    Ok(UserInfo {
        id: row.get(0)?,
        username: row.get(1)?,
        created_at: row.get(2)?,
        last_login: row.get(3)?,
    })
}

/// Hash a password into an Argon2id PHC string (algorithm + params + random salt
/// + hash, all in one self-describing token).
fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| CoreError::Config(format!("hash password: {e}")))
}

/// Verify a password against a stored PHC hash. A malformed stored hash verifies
/// as `false` rather than erroring, so a corrupt row can't lock everyone out in a
/// way that looks like a crash.
fn verify_password(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Validate + normalize a username (trimmed). Empty names are rejected.
fn normalize_username(username: &str) -> Result<String> {
    let u = username.trim();
    if u.is_empty() {
        return Err(CoreError::Config("Username cannot be empty.".into()));
    }
    if u.chars().count() > 64 {
        return Err(CoreError::Config("Username is too long (max 64).".into()));
    }
    Ok(u.to_string())
}

/// Whether the app lock is set up yet (i.e. at least one account exists). When
/// this is false the UI should show first-run setup rather than a login screen.
pub fn is_configured(conn: &Connection) -> Result<bool> {
    Ok(user_count(conn)? > 0)
}

/// How many accounts exist.
pub fn user_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)
}

/// All accounts, oldest first (no password material).
pub fn list_users(conn: &Connection) -> Result<Vec<UserInfo>> {
    let mut stmt = conn.prepare(
        "SELECT id, username, created_at, last_login FROM users ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], read_user)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Create a new account. Fails if the username is taken or the password is too
/// short. The password is hashed before it ever touches the database.
pub fn create_user(conn: &Connection, username: &str, password: &str) -> Result<UserInfo> {
    let username = normalize_username(username)?;
    if password.len() < MIN_PASSWORD_LEN {
        return Err(CoreError::Config(format!(
            "Password must be at least {MIN_PASSWORD_LEN} characters."
        )));
    }
    // Case-insensitive uniqueness (the column is COLLATE NOCASE, but check first
    // for a friendly error instead of a raw constraint failure).
    let taken: bool = conn
        .query_row(
            "SELECT 1 FROM users WHERE username = ?1",
            [&username],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(CoreError::Config(format!(
            "An account named \"{username}\" already exists."
        )));
    }

    let hash = hash_password(password)?;
    let ts = now();
    conn.execute(
        "INSERT INTO users (username, password_hash, created_at, last_login)
         VALUES (?1, ?2, ?3, NULL)",
        params![username, hash, ts],
    )?;
    let id = conn.last_insert_rowid();
    Ok(UserInfo {
        id,
        username,
        created_at: ts,
        last_login: None,
    })
}

/// Verify a username + password. On success, stamps `last_login` and returns the
/// account; on failure returns `Ok(None)` (indistinguishable whether the username
/// or the password was wrong, so we don't reveal which accounts exist).
pub fn verify_credentials(
    conn: &Connection,
    username: &str,
    password: &str,
) -> Result<Option<UserInfo>> {
    let username = username.trim();
    let row: Option<(i64, String, i64, Option<i64>, String)> = conn
        .query_row(
            "SELECT id, username, created_at, last_login, password_hash
             FROM users WHERE username = ?1",
            [username],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                ))
            },
        )
        .optional()?;

    let Some((id, uname, created_at, _last, hash)) = row else {
        return Ok(None);
    };
    if !verify_password(password, &hash) {
        return Ok(None);
    }

    let ts = now();
    conn.execute("UPDATE users SET last_login = ?2 WHERE id = ?1", params![id, ts])?;
    Ok(Some(UserInfo {
        id,
        username: uname,
        created_at,
        last_login: Some(ts),
    }))
}

/// Change a user's password after verifying the current one. Returns
/// `Ok(false)` if the current password is wrong (leaving the old one in place).
pub fn change_password(
    conn: &Connection,
    username: &str,
    current: &str,
    new: &str,
) -> Result<bool> {
    if new.len() < MIN_PASSWORD_LEN {
        return Err(CoreError::Config(format!(
            "New password must be at least {MIN_PASSWORD_LEN} characters."
        )));
    }
    let username = username.trim();
    let hash: Option<String> = conn
        .query_row(
            "SELECT password_hash FROM users WHERE username = ?1",
            [username],
            |r| r.get(0),
        )
        .optional()?;
    let Some(hash) = hash else {
        return Err(CoreError::NotFound(format!("user {username}")));
    };
    if !verify_password(current, &hash) {
        return Ok(false);
    }
    let new_hash = hash_password(new)?;
    conn.execute(
        "UPDATE users SET password_hash = ?2 WHERE username = ?1",
        params![username, new_hash],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn first_run_is_unconfigured() {
        let db = Database::open_in_memory().unwrap();
        assert!(!is_configured(db.conn()).unwrap());
        assert_eq!(user_count(db.conn()).unwrap(), 0);
    }

    #[test]
    fn create_then_login() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let u = create_user(conn, "  Frixxy ", "supersecret").unwrap();
        assert_eq!(u.username, "Frixxy"); // trimmed
        assert!(is_configured(conn).unwrap());

        // Correct password logs in and stamps last_login.
        let ok = verify_credentials(conn, "Frixxy", "supersecret").unwrap();
        assert!(ok.is_some());
        assert!(ok.unwrap().last_login.is_some());

        // Username match is case-insensitive.
        assert!(verify_credentials(conn, "frixxy", "supersecret").unwrap().is_some());

        // Wrong password fails; unknown user fails — both as None, not error.
        assert!(verify_credentials(conn, "Frixxy", "wrong").unwrap().is_none());
        assert!(verify_credentials(conn, "nobody", "supersecret").unwrap().is_none());
    }

    #[test]
    fn password_is_hashed_not_plaintext() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        create_user(conn, "artist", "hunter2!!").unwrap();
        let stored: String = conn
            .query_row("SELECT password_hash FROM users WHERE username='artist'", [], |r| r.get(0))
            .unwrap();
        assert!(stored.starts_with("$argon2"), "must be an Argon2 PHC string");
        assert!(!stored.contains("hunter2"), "plaintext must never be stored");
    }

    #[test]
    fn rejects_duplicate_and_short_password() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        create_user(conn, "artist", "goodpass").unwrap();
        // Duplicate (case-insensitive) is rejected.
        assert!(create_user(conn, "ARTIST", "otherpass").is_err());
        // Too-short password is rejected.
        assert!(create_user(conn, "second", "abc").is_err());
        // Empty username is rejected.
        assert!(create_user(conn, "   ", "goodpass").is_err());
    }

    #[test]
    fn change_password_flow() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        create_user(conn, "artist", "oldpass1").unwrap();

        // Wrong current password → false, nothing changes.
        assert!(!change_password(conn, "artist", "nope", "newpass1").unwrap());
        assert!(verify_credentials(conn, "artist", "oldpass1").unwrap().is_some());

        // Correct current password → rotates the hash.
        assert!(change_password(conn, "artist", "oldpass1", "newpass1").unwrap());
        assert!(verify_credentials(conn, "artist", "oldpass1").unwrap().is_none());
        assert!(verify_credentials(conn, "artist", "newpass1").unwrap().is_some());

        // Too-short new password is rejected.
        assert!(change_password(conn, "artist", "newpass1", "x").is_err());
    }
}
