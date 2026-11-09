use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub const MAX_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024; // 2GB，所有用户共同上限
pub const GUEST_FRAME_LIMIT_BYTES: u64 = 30 * 1024 * 1024; // 游客抽帧 30MB
pub const GUEST_MAX_CUT_POINTS: u32 = 2; // 游客最多 2 个切割点
pub const MAX_EXPORT_SEGMENTS: u32 = 40; // 注册用户单次导出片段上限，避免误操作
const SESSION_DAYS: i64 = 30;

#[derive(Debug, Clone)]
pub enum Identity {
    Guest,
    User {
        #[allow(dead_code)]
        id: i64,
        username: String,
    },
}

impl Identity {
    pub fn is_guest(&self) -> bool {
        matches!(self, Identity::Guest)
    }

    pub fn username(&self) -> Option<String> {
        match self {
            Identity::Guest => None,
            Identity::User { username, .. } => Some(username.clone()),
        }
    }
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn open_database(data_dir: &Path) -> Result<Connection, String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let path = data_dir.join("dc_video_cut.db");
    let conn = Connection::open(&path).map_err(|e| format!("打开数据库失败: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS users (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             username TEXT NOT NULL UNIQUE,
             display_name TEXT NOT NULL,
             password_hash TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS sessions (
             token TEXT PRIMARY KEY,
             user_id INTEGER NOT NULL,
             created_at INTEGER NOT NULL,
             expires_at INTEGER NOT NULL,
             FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
         );
         CREATE TABLE IF NOT EXISTS settings (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );",
    )
    .map_err(|e| format!("初始化数据库失败: {e}"))?;
    Ok(conn)
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("密码加密失败: {e}"))
}

fn verify_password(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

fn validate_credentials(username: &str, password: &str) -> Result<(), String> {
    let name = username.trim();
    let len = name.chars().count();
    if !(3..=24).contains(&len) {
        return Err("用户名长度需为 3-24 个字符".into());
    }
    if name.chars().any(|c| c.is_whitespace()) {
        return Err("用户名不能包含空格".into());
    }
    if name.chars().any(|c| c.is_control() || c == '/' || c == '\\') {
        return Err("用户名包含非法字符".into());
    }
    if password.chars().count() < 6 {
        return Err("密码至少需要 6 位".into());
    }
    if password.len() > 128 {
        return Err("密码过长（最多 128 位）".into());
    }
    Ok(())
}

fn create_session(conn: &Connection, user_id: i64) -> Result<String, String> {
    let token = Uuid::new_v4().simple().to_string();
    let now = now_ts();
    conn.execute(
        "DELETE FROM sessions WHERE expires_at < ?1",
        params![now],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO sessions (token, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![token, user_id, now, now + SESSION_DAYS * 86400],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('session_token', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![token],
    )
    .map_err(|e| e.to_string())?;
    Ok(token)
}

pub fn register(
    conn: &Connection,
    username: &str,
    password: &str,
) -> Result<Identity, String> {
    validate_credentials(username, password)?;
    let name = username.trim().to_string();
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM users WHERE lower(username) = lower(?1)",
            params![name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if exists.is_some() {
        return Err("该用户名已被注册，请换一个".into());
    }
    let hash = hash_password(password)?;
    conn.execute(
        "INSERT INTO users (username, display_name, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![name.to_lowercase(), name, hash, now_ts()],
    )
    .map_err(|e| e.to_string())?;
    let id: i64 = conn
        .query_row(
            "SELECT id FROM users WHERE username = ?1",
            params![name.to_lowercase()],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    create_session(conn, id)?;
    Ok(Identity::User { id, username: name })
}

pub fn login(conn: &Connection, username: &str, password: &str) -> Result<Identity, String> {
    if username.trim().is_empty() || password.is_empty() {
        return Err("请输入用户名和密码".into());
    }
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, display_name, password_hash FROM users WHERE lower(username) = lower(?1)",
            params![username.trim().to_lowercase()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (id, display_name, hash) = match row {
        Some(v) => v,
        None => return Err("用户名或密码不正确".into()),
    };
    if !verify_password(password, &hash) {
        return Err("用户名或密码不正确".into());
    }
    create_session(conn, id)?;
    Ok(Identity::User {
        id,
        username: display_name,
    })
}

pub fn restore_session(conn: &Connection) -> Option<Identity> {
    let token: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'session_token'",
            [],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let token = token?;
    let row: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT u.id, u.display_name, s.expires_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1",
            params![token],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .ok()
        .flatten();
    match row {
        Some((id, username, expires_at)) if expires_at > now_ts() => {
            Some(Identity::User { id, username })
        }
        _ => {
            let _ = conn.execute("DELETE FROM settings WHERE key = 'session_token'", []);
            None
        }
    }
}

pub fn logout(conn: &Connection) -> Result<(), String> {
    conn.execute("DELETE FROM settings WHERE key = 'session_token'", [])
        .map_err(|e| e.to_string())?;
    if let Some(token) = current_token(conn) {
        let _ = conn.execute("DELETE FROM sessions WHERE token = ?1", params![token]);
    }
    Ok(())
}

fn current_token(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = 'session_token'",
        [],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}
