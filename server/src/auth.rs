use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

const HOUR: u64 = 3600;
const MAX_WAIT: u64 = 60;

pub struct RateLimiter {
    fails: Mutex<HashMap<String, (u32, Instant)>>,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            fails: Mutex::new(HashMap::new()),
            now: Box::new(Instant::now),
        }
    }

    // per i test: orologio iniettabile, il tempo simula l'avanzamento senza sleep
    pub fn with_clock(now: Box<dyn Fn() -> Instant + Send + Sync>) -> Self {
        Self {
            fails: Mutex::new(HashMap::new()),
            now,
        }
    }

    pub fn reap(&self) {
        // voci vecchie di 1 h escono dal map (evita accumulo per email inventate)
        let now = (self.now)();
        self.fails
            .lock()
            .unwrap()
            .retain(|_, (_, last)| now.duration_since(*last).as_secs() < HOUR);
    }

    pub fn check(&self, key: &str) -> Result<(), u64> {
        let m = self.fails.lock().unwrap();
        if let Some(&(n, last)) = m.get(key) {
            if n >= 5 {
                let wait = (1u64 << (n - 5).min(14)).min(MAX_WAIT);
                let elapsed = (self.now)().duration_since(last).as_secs();
                if elapsed < wait {
                    return Err(wait - elapsed);
                }
            }
        }
        Ok(())
    }

    pub fn fail(&self, key: &str) {
        let mut m = self.fails.lock().unwrap();
        let now = (self.now)();
        let e = m
            .entry(key.to_string())
            .and_modify(|e| e.1 = now)
            .or_insert((0, now));
        e.0 += 1;
    }

    pub fn success(&self, key: &str) {
        self.fails.lock().unwrap().remove(key);
    }
}
// login: due chiavi, IP (ultimo XFF) e email ("mail:<email>"); 429 se UNA delle
// due è in attesa → .and(), MAI .or (che bloccherebbe solo se entrambe in attesa).

pub const COOKIE_MAX_AGE: i64 = 30 * 24 * 3600;

pub fn new_token() -> String {
    use rand::Rng;
    let b: [u8; 32] = rand::random();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn token_hash(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(token.as_bytes());
    h.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn cookie_for(token: &str) -> String {
    format!("session={token}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={COOKIE_MAX_AGE}")
}

pub fn session_token_from_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let c = headers.get("cookie")?.to_str().ok()?;
    c.split(';')
        .filter_map(|s| s.trim().strip_prefix("session="))
        .next()
        .map(|t| t.to_string())
}

pub fn mime_of(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "txt" | "md" | "rs" | "toml" | "csv" => "text/plain",
        _ => "application/octet-stream",
    }
}

pub fn hash_sync(pw: &str) -> String {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

pub fn verify_sync(phc: &str, pw: &str) -> bool {
    PasswordHash::new(phc)
        .ok()
        .map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

pub async fn hash(pw: String) -> String {
    tokio::task::spawn_blocking(move || hash_sync(&pw))
        .await
        .unwrap()
}

pub async fn verify(phc: String, pw: String) -> bool {
    tokio::task::spawn_blocking(move || verify_sync(&phc, &pw))
        .await
        .unwrap()
}
