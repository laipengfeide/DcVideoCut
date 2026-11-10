use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand_core::{OsRng, RngCore};
use rusqlite::serialize::OwnedData;
use rusqlite::{params, Connection, DatabaseName, OptionalExtension};
use uuid::Uuid;

pub const MAX_VIDEO_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const GUEST_FRAME_LIMIT_BYTES: u64 = 30 * 1024 * 1024;
pub const GUEST_MAX_CUT_POINTS: u32 = 2;
pub const MAX_EXPORT_SEGMENTS: u32 = 40;
const SESSION_DAYS: i64 = 30;

const DB_FILE: &str = "dc_video_cut.db";
/// 数据库密钥文件（与 db 同目录），首行为数据库密码，由本机用户自行保管
const DB_KEY_FILE: &str = "db.key";
const DB_MAGIC: &[u8; 8] = b"DCVCENC1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 8 + SALT_LEN + NONCE_LEN;

/// 整库加密（AES-256-GCM）：主密钥由 db.key 中的密码经 Argon2id 派生
pub struct DbCrypto {
    key: [u8; 32],
    salt: [u8; SALT_LEN],
    path: PathBuf,
}

impl DbCrypto {
    fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        let mut key = [0u8; 32];
        Argon2::default()
            .hash_password_into(passphrase.as_bytes(), salt, &mut key)
            .map_err(|e| format!("数据库密钥派生失败: {e}"))?;
        Ok(key)
    }

    fn create(passphrase: &str, path: PathBuf) -> Result<Self, String> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let key = Self::derive_key(passphrase, &salt)?;
        Ok(Self { key, salt, path })
    }

    /// 解密磁盘上的密文库，返回 (加密器, 明文数据库字节)
    fn unlock(passphrase: &str, raw: &[u8], path: PathBuf) -> Result<(Self, Vec<u8>), String> {
        if raw.len() <= HEADER_LEN {
            return Err("数据库文件已损坏".into());
        }
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&raw[8..8 + SALT_LEN]);
        let nonce = Nonce::from_slice(&raw[8 + SALT_LEN..HEADER_LEN]);
        let key = Self::derive_key(passphrase, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("初始化加密器失败: {e}"))?;
        let plain = cipher
            .decrypt(nonce, &raw[HEADER_LEN..])
            .map_err(|_| "数据库解密失败：密钥文件与数据库不匹配或文件已损坏".to_string())?;
        Ok((Self { key, salt, path }, plain))
    }

    /// 将内存数据库整体序列化后加密写盘（原子替换）
    pub fn save(&self, conn: &Connection) -> Result<(), String> {
        let data = conn
            .serialize(DatabaseName::Main)
            .map_err(|e| format!("导出数据库失败: {e}"))?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|e| format!("初始化加密器失败: {e}"))?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), &*data)
            .map_err(|_| "加密数据库失败".to_string())?;

        let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        out.extend_from_slice(DB_MAGIC);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);

        let tmp = self.path.with_extension("db.tmp");
        fs::write(&tmp, &out).map_err(|e| format!("写入数据库失败: {e}"))?;
        fs::rename(&tmp, &self.path).map_err(|e| format!("保存数据库失败: {e}"))?;
        Ok(())
    }
}

/// 读取 db.key 中的数据库密码；首次运行时生成随机密钥并落盘
fn load_passphrase(data_dir: &Path) -> Result<String, String> {
    let key_path = data_dir.join(DB_KEY_FILE);
    if key_path.exists() {
        let raw = fs::read_to_string(&key_path).map_err(|e| format!("读取数据库密钥失败: {e}"))?;
        let pass = raw.trim().to_string();
        if pass.is_empty() {
            return Err(format!("数据库密钥文件为空: {}", key_path.display()));
        }
        return Ok(pass);
    }
    let mut bytes = [0u8; 24];
    OsRng.fill_bytes(&mut bytes);
    let pass: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    fs::write(&key_path, &pass).map_err(|e| format!("创建数据库密钥失败: {e}"))?;
    Ok(pass)
}

fn load_plain_into(conn: &mut Connection, bytes: &[u8]) -> Result<(), String> {
    let size = bytes.len();
    if size == 0 {
        return Err("数据库内容为空".into());
    }
    unsafe {
        let ptr = rusqlite::ffi::sqlite3_malloc64(size as u64) as *mut u8;
        let nn = NonNull::new(ptr).ok_or_else(|| "分配数据库内存失败".to_string())?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, size);
        let owned = OwnedData::from_raw_nonnull(nn, size);
        conn.deserialize(DatabaseName::Main, owned, false)
            .map_err(|e| format!("载入数据库失败: {e}"))?;
    }
    Ok(())
}

/// 旧版明文库整理：把 WAL 内容合并回主文件并切回转存模式
fn normalize_legacy(path: &Path) -> Result<(), String> {
    let legacy = Connection::open(path).map_err(|e| format!("打开旧数据库失败: {e}"))?;
    legacy
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode = DELETE;")
        .map_err(|e| format!("整理旧数据库失败: {e}"))?;
    drop(legacy);
    Ok(())
}

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

/// 打开（必要时解密/迁移）数据库，返回内存连接与整库加密器
pub fn open_database(data_dir: &Path) -> Result<(Connection, DbCrypto), String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let path = data_dir.join(DB_FILE);
    let passphrase = load_passphrase(data_dir)?;
    let mut conn =
        Connection::open_in_memory().map_err(|e| format!("创建数据库失败: {e}"))?;

    let crypto = if path.exists() {
        let raw = fs::read(&path).map_err(|e| format!("读取数据库失败: {e}"))?;
        if raw.starts_with(DB_MAGIC) {
            let (crypto, plain) = DbCrypto::unlock(&passphrase, &raw, path)?;
            load_plain_into(&mut conn, &plain)?;
            crypto
        } else {
            // 旧版明文库：整理 WAL 后整库加密接管
            normalize_legacy(&path)?;
            let plain = fs::read(&path).map_err(|e| format!("读取旧数据库失败: {e}"))?;
            load_plain_into(&mut conn, &plain)?;
            DbCrypto::create(&passphrase, path)?
        }
    } else {
        DbCrypto::create(&passphrase, path)?
    };

    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
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

    // 确保磁盘上是加密文件（首次运行或旧库迁移后）
    crypto.save(&conn)?;
    Ok((conn, crypto))
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("密码加密失败: {e}"))
}
fn verify_password(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .map(|p| Argon2::default().verify_password(password.as_bytes(), &p).is_ok())
        .unwrap_or(false)
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
    let _ = conn.execute("DELETE FROM sessions WHERE expires_at < ?1", params![now]);
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
pub fn register(conn: &Connection, username: &str, password: &str) -> Result<Identity, String> {
    validate_credentials(username, password)?;
    let name = username.trim().to_string();
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM users WHERE lower(username) = lower(?1)",
            params![name],
            |r| r.get(0),
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
            |r| r.get(0),
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
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
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
    Ok(Identity::User { id, username: display_name })
}
pub fn restore_session(conn: &Connection) -> Option<Identity> {
    let token: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'session_token'",
            [],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let row: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT u.id, u.display_name, s.expires_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1",
            params![token],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .ok()
        .flatten();
    match row {
        Some((id, username, exp)) if exp > now_ts() => {
            Some(Identity::User { id, username })
        }
        _ => {
            let _ = conn.execute("DELETE FROM settings WHERE key = 'session_token'", []);
            None
        }
    }
}
pub fn logout(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "DELETE FROM settings WHERE key = 'session_token'",
        [],
    )
    .map_err(|e| e.to_string())?;
    if let Ok(Some(token)) = conn.query_row(
        "SELECT value FROM settings WHERE key = 'session_token'",
        [],
        |r| r.get::<_, String>(0),
    ).optional() {
        let _ = conn.execute("DELETE FROM sessions WHERE token = ?1", params![token]);
    }
    Ok(())
}
