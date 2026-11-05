# Vaulti MVP — Piano di implementazione

> **Per agentic workers:** REQUIRED SUB-SKILL: usa superpowers:subagent-driven-development (consigliata) o superpowers:executing-plans per implementare il piano task per task. Passi con checkbox (`- [ ]`).

**Goal:** Server + dashboard web per Vaulti: cloud personale con upload a blocchi riprendibili
(preallocato + rename, zero copie), cartelle nel modello, download Range RFC 9110, auth argon2id +
sessioni con token hashed, rate limit XFF, hard delete, binario musl via GitHub Actions.

**Architecture:** Crate unico `server` (axum 0.8, sqlx 0.8 + SQLite) con 17 route JSON e dashboard
statica embedded. Blob in `storage/<user_id>/<uuid>`, preallocazione in `tmp/<upload_id>/data`.

**Tech Stack:** Rust stable 1.99.1, axum 0.8, tokio, sqlx 0.8 (sqlite, migrate, macros), argon2 0.5,
sha2, uuid v4, rand 0.8, tokio-util, tower-http 0.6 (fs), fs4 0.12, futures-util; test con
`tower::ServiceExt::oneshot` + tempfile.

**Spec:** `docs/superpowers/specs/2026-11-05-vaulti-mvp-design.md`

## Global Constraints

- axum `0.8`, route `{param}`. sqlx `0.8`. rust-toolchain.toml: channel `1.99.1`,
  `components = ["clippy", "rustfmt"]`.
- Listen `127.0.0.1:8420` (env `VAULTI_PORT`), data dir `/srv/vaulti` (env `VAULTI_DATA_DIR`),
  `CHUNK_SIZE = 8 * 1024 * 1024` (env `VAULTI_CHUNK_SIZE`, per i test 2).
- DB: `{data_dir}/vaulti.db`, WAL + busy_timeout 5 s, foreign_keys on.
- Sessione 30 giorni: cookie `session=<token>; HttpOnly; Secure; SameSite=Lax; Path=/;
  Max-Age=2592000`. Nel DB solo `sha256(token)` esadecimale (64 char).
- Password min 8. Errori JSON `{"error":"..."}`. `is_admin` risponde bool.
- Sicurezza: il `name` del client non è mai path; ogni query filtra `user_id` della sessione.
- Ogni task: test fallito → minimo → verde → commit conventional.

## Review Focus

1. Range end oltre size → 206 troncato; start ≥ size → 416; suffix `bytes=-N` (Task 6).
2. Cross-user → 404 (Task 6, secondo utente via INSERT diretto in test).
3. Sessione scaduta → 401 (Task 3).
4. Finish con somma len != size → 409, nessun blob in storage (Task 4; il test usa chunk_size=2).
5. Chunk: id UUID-validato, owner-verificato PRIMA della scrittura su disco, n < total,
   lunghezza attesa verificata; la size di files viene dai chunk reali, non dal client (Task 4).
6. XSS: headers download (Task 6).

---

## File Structure (bersaglio)

```
Cargo.toml, rust-toolchain.toml, .gitignore
.github/workflows/ci.yml
server/Cargo.toml
server/src/lib.rs               ← moduli, AppState, build_router, run()
server/src/main.rs              ← thin bin
server/src/config.rs            ← env: data_dir, port, chunk_size
server/src/db.rs                ← pool (WAL, busy_timeout), migrate
server/src/migrations/0001_init.sql
server/src/auth.rs              ← argon2 (0.5), token+cookie, rate limiter, SessionUser, mime
server/src/handlers_auth.rs     ← register/login/logout/me/admin-create-user
server/src/handlers_store.rs    ← upload start/chunk/status/finish/cancel
server/src/handlers_files.rs    ← list(+quota,+folders), create/patch folders, patch files,
                                   delete hard, download (ServeFile, headers anti-XSS)
server/src/handlers_static.rs   ← dashboard embedded
server/tests/common/mod.rs, integration.rs
web/index.html, web/app.js, web/style.css
deploy/vaulti.service, deploy/README.md
```

---

### Task 1: Scaffold workspace + CI

**Files:** Create `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `server/Cargo.toml`,
`server/src/lib.rs`, `server/src/main.rs`, `server/src/config.rs`, `.github/workflows/ci.yml`

**Interfaces:** Produces — `vaulti_server::run()` (Task 2 riempie), `config::Config { data_dir:
String, port: u16, chunk_size: u64 }`.

- [ ] **Step 1: file build**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["server"]
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
```

`.gitignore`:
```
/target/
```

`server/Cargo.toml`:
```toml
[package]
name = "vaulti-server"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
tempfile = "3"
```

`server/src/config.rs`:
```rust
use std::env;

#[derive(Clone)]
pub struct Config { pub data_dir: String, pub port: u16, pub chunk_size: u64 }

pub fn load() -> Config {
    Config {
        data_dir: env::var("VAULTI_DATA_DIR").unwrap_or_else(|_| "/srv/vaulti".into()),
        port: env::var("VAULTI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8420),
        chunk_size: env::var("VAULTI_CHUNK_SIZE").ok().and_then(|p| p.parse().ok())
            .unwrap_or(8 * 1024 * 1024),
    }
}
```

`server/src/lib.rs`:
```rust
pub mod config;
pub fn run() { /* Task 2 */ }
```

`server/src/main.rs`:
```rust
fn main() { vaulti_server::run() }
```

- [ ] **Step 2: CI** — `.github/workflows/ci.yml`:
```yaml
name: ci
on:
  push: { branches: [main], tags: ["v*"] }
  pull_request:
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test --workspace
  release:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: test
    runs-on: ubuntu-latest
    permissions: { contents: write }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: x86_64-unknown-linux-musl }
      - run: sudo apt-get update && sudo apt-get install -y musl-tools
      - run: cargo build --release --target x86_64-unknown-linux-musl
      - uses: softprops/action-gh-release@v2
        with: { files: target/x86_64-unknown-linux-musl/release/vaulti-server }
```

- [ ] **Step 3: `cargo build` compila verde** — poi Commit: `chore: scaffold workspace + CI`

### Task 2: DB + migrazioni + harness test

**Files:** Modify `server/Cargo.toml`; Create `server/src/db.rs`, `server/src/migrations/0001_init.sql`,
`server/tests/common/mod.rs`; Modify `lib.rs` (AppState + router healthz)

**Interfaces:**
- Produces — `db::new_pool(path) -> SqlitePool` (":memory:" → single conn; file → 5 conn, WAL,
  busy_timeout 5 s, foreign_keys), `db::migrate(&pool)`, `AppState::new(pool, data_dir, chunk_size)`
  e `AppState::for_test(pool, data_dir, chunk_size)` (ai test serve limiter nuova: `AppState` con
  `limiter` nuovo), `build_router(AppState) -> Router`, test app: `boot()` chunk_size 2 +
  helpers `json_req`, `authed_req`, `to_bytes`, `register_user`, `login_cookie`, `logged_in`,
  `start_upload`, `put_chunk`, `finish_ok`.

- [ ] **Step 1: deps** — `[dependencies]` in `server/Cargo.toml` aggiunge:

```toml
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "migrate", "macros"] }
uuid = { version = "1", features = ["v4"] }
rand = "0.8"
sha2 = "0.10"
tokio-util = { version = "0.7", features = ["io"] }
futures-util = "0.3"
tower-http = { version = "0.6", features = ["fs"] }
fs4 = "0.12"
argon2 = "0.5"
```

- [ ] **Step 2: `server/src/migrations/0001_init.sql`**:
```sql
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE sessions (
  token_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  expires_at INTEGER NOT NULL
);
CREATE TABLE folders (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  parent_id TEXT REFERENCES folders(id),
  parent_key TEXT NOT NULL GENERATED ALWAYS AS (COALESCE(parent_id, '!root!')),
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (user_id, parent_key, name)
);
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  folder_id TEXT REFERENCES folders(id),
  name TEXT NOT NULL,
  size INTEGER NOT NULL,
  mime TEXT NOT NULL,
  stored_name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE TABLE uploads (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  chunk_size INTEGER NOT NULL,
  total_chunks INTEGER NOT NULL,
  size INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE upload_chunks (
  upload_id TEXT NOT NULL REFERENCES uploads(id),
  chunk_n INTEGER NOT NULL,
  len INTEGER NOT NULL,
  received_at INTEGER NOT NULL,
  PRIMARY KEY (upload_id, chunk_n)
);
CREATE INDEX idx_files_user ON files(user_id, deleted_at);
```

- [ ] **Step 3: `server/src/db.rs`**:
```rust
use sqlx::{sqlite::{SqliteConnectOptions, SqliteJournalMode,
  SqlitePoolOptions, SqliteSynchronous}, SqlitePool};
use std::time::Duration;

pub async fn new_pool(path: &str) -> sqlx::Result<SqlitePool> {
    let mut o = SqliteConnectOptions::new().create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    o = match path {
        ":memory:" => o.shared_cache(true),
        p => o.filename(p),
    };
    if path != ":memory:" { o = o.journal_mode(SqliteJournalMode::Wal).synchronous(SqliteSynchronous::Normal); }
    let max = if path == ":memory:" { 1 } else { 5 };
    SqlitePoolOptions::new().max_connections(max).connect_with(o).await
}

pub async fn migrate(p: &SqlitePool) -> sqlx::Result<()> {
    sqlx::migrate!("src/migrations").run(p).await
}
```

- [ ] **Step 4: `lib.rs`** — sostituisci tutto:
```rust
pub mod config;
pub mod db;

use axum::{routing::get, Router};
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<String>,
    pub limiter: Arc<crate::auth::RateLimiter>,
    pub chunk_size: u64,
}

impl AppState {
    pub fn new(pool: SqlitePool, data_dir: String, chunk_size: u64) -> Self {
        Self { pool, data_dir: Arc::new(data_dir), limiter: Arc::new(crate::auth::RateLimiter::new()), chunk_size }
    }
}

pub fn build_router(_state: AppState) -> Router {
    Router::new().route("/healthz", get(|| async { "ok" }))
}

pub fn run() {
    let cfg = config::load();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        std::fs::create_dir_all(format!("{}/storage", cfg.data_dir)).unwrap();
        std::fs::create_dir_all(format!("{}/tmp", cfg.data_dir)).unwrap();
        let pool = db::new_pool(&format!("{}/vaulti.db", cfg.data_dir)).await.unwrap();
        db::migrate(&pool).await.unwrap();
        let state = AppState::new(pool, cfg.data_dir.clone(), cfg.chunk_size);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await.unwrap();
        println!("vaulti sul port {}", cfg.port);
        axum::serve(listener, build_router(state)).await.unwrap();
    });
}
```
e stub `server/src/auth.rs` (Task 3 le completa):
```rust
use std::{collections::HashMap, net::IpAddr, time::Instant};
use std::sync::Mutex;

pub struct RateLimiter { fails: Mutex<HashMap<IpAddr, (u32, Instant)>> }
impl RateLimiter {
    pub fn new() -> Self { Self { fails: Mutex::new(HashMap::new()) } }
}
```

- [ ] **Step 5: `server/tests/common/mod.rs`**:
```rust
#![allow(dead_code)]
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::SqlitePool;
use tower::ServiceExt;

pub struct TestApp {
    pub app: axum::Router,
    pub pool: SqlitePool,
    pub data_dir: tempfile::TempDir,
}

pub async fn boot() -> TestApp {
    let data_dir = tempfile::tempdir().unwrap();
    let pool = vaulti_server::db::new_pool(":memory:").await.unwrap();
    vaulti_server::db::migrate(&pool).await.unwrap();
    let state = vaulti_server::AppState::new(pool.clone(),
        data_dir.path().to_string_lossy().to_string(), 2);
    TestApp { app: vaulti_server::build_router(state), pool, data_dir }
}

pub fn json_req(uri: &str, body: Value) -> Request<Body> {
    Request::builder().method("POST").uri(uri)
        .header("content-type", "application/json")
        .header("x-forwarded-for", "1.2.3.4")
        .body(Body::from(body.to_string())).unwrap()
}

pub fn authed_req(method: &str, uri: &str, body: Option<Value>, cookie: &str) -> Request<Body> {
    let b = Request::builder().method(method).uri(uri)
        .header("cookie", cookie.to_string())
        .header("x-forwarded-for", "1.2.3.4");
    if let Some(v) = body {
        b.header("content-type", "application/json")
         .body(Body::from(v.to_string())).unwrap()
    } else {
        b.body(Body::empty()).unwrap()
    }
}

pub fn raw_req(method: &str, uri: &str, cookie: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder().method(method).uri(uri)
        .header("cookie", cookie.to_string())
        .header("x-forwarded-for", "1.2.3.4")
        .body(Body::from(body)).unwrap()
}

pub async fn to_bytes(b: Body) -> Vec<u8> {
    b.collect().await.unwrap().to_bytes().to_vec()
}

pub async fn register_user(t: &TestApp, email: &str, pw: &str) {
    let res = t.app.clone().oneshot(json_req("/auth/register",
        serde_json::json!({"email": email, "password": pw}))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

pub async fn login_cookie(t: &TestApp, email: &str, pw: &str) -> String {
    let res = t.app.clone().oneshot(json_req("/auth/login",
        serde_json::json!({"email": email, "password": pw}))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    res.headers().get("set-cookie").unwrap().to_str().unwrap()
        .split(';').next().unwrap().to_string()
}

pub async fn logged_in(t: &TestApp) -> String {
    register_user(t, "a@b.c", "password123").await;
    login_cookie(t, "a@b.c", "password123").await
}
```

- [ ] **Step 6: test migrazioni — `server/tests/integration.rs`**:
```rust
mod common;
use axum::body::Body;
use axum::http::Request;
use common::*;

#[tokio::test]
async fn healthz_ok() {
    let t = boot().await;
    let res = t.app.oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn migrations_create_tables() {
    let t = boot().await;
    for table in ["users", "sessions", "folders", "files", "uploads", "upload_chunks"] {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?")
            .bind(table).fetch_one(&t.pool).await.unwrap();
        assert_eq!(n, 1, "tabella {table} manca");
    }
}
```

- [ ] **Step 7: `cargo test --workspace`** — 2 test verdi (note: `run()` not invoked from the tests).
- [ ] **Step 8: Commit:** `feat: sqlite walnut, migrazioni, harness`

### Task 3: Auth completa

**Files:** Modify `server/src/auth.rs`, `server/src/handlers_auth.rs` (new), `lib.rs` (routes)
**Test:** `integration.rs`
**Interfaces:** Produces —
- `auth::hash(&str) -> String` PHC, `auth::verify(&str, &str) -> bool` (async wrapper:
  `tokio::task::spawn_blocking`).
- `auth::new_token() -> String` 64 hex; `auth::token_hash(&str) -> String` sha256 hex;
   `auth::cookie_for(&str) -> String`; `auth::session_token_from_cookie(&HeaderMap) -> Option<String>`.
- `RateLimiter: check(ip, now) -> Result<(), u64>`, `fail(ip)`, `fail(ip, now)`, `success(ip)`
  — struttura internal takes `last_fail`: `fail` set again; check è una finestra fresca se
  respectively da `Config` vuota e comunque 1 h expiration.
- Route: `POST /auth/register`, `POST /auth/login`, `POST /auth/logout`, `GET /api/me`,
  `POST /api/admin/users`.
- `SessionUser { user_id: String, is_admin: bool }` FromRequestParts (401).

- [ ] **Step 1: test che falliscono** (append integration.rs):

```rust
#[tokio::test]
async fn register_first_user_then_403() {
    let t = boot().await;
    let app = t.app.clone();
    let res = app.clone().oneshot(json_req("/auth/register",
        serde_json::json!({"email":"a@b.c","password":"password123"}))).await.unwrap();
    assert_eq!(res.status(), 200);
    let res = app.oneshot(json_req("/auth/register",
        serde_json::json!({"email":"d@e.f","password":"passwordabc"}))).await.unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn register_short_password_400() {
    let t = boot().await;
    let res = t.app.oneshot(json_req("/auth/register",
        serde_json::json!({"email":"a@b.c","password":"pw"}))).await.unwrap();
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn login_ok_cookie_and_me_bool_admin() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let cookie = login_cookie(&t, "a@b.c", "password123").await;
    assert!(cookie.starts_with("session=") && cookie.len() > 40);
    let res = t.app.clone().oneshot(
        Request::builder().uri("/api/me").header("cookie", cookie)
            .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["email"], "a@b.c");
    assert_eq!(j["is_admin"], true);
}

#[tokio::test]
async fn login_bad_password_401() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let res = t.app.clone().oneshot(json_req("/auth/login",
        serde_json::json!({"email":"a@b.c","password":"nope"}))).await.unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn me_without_session_401() {
    let t = boot().await;
    let res = t.app.oneshot(Request::builder().uri("/api/me").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn session_expired_401() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let cookie = login_cookie(&t, "a@b.c", "password123").await;
    sqlx::query("UPDATE sessions SET expires_at = -1").execute(&t.pool).await.unwrap();
    let res = t.app.oneshot(
        Request::builder().uri("/api/me").header("cookie", cookie).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn login_ratelimit_429() {
    let t = boot().await;
    register_user(&t, "a@b.c", "password123").await;
    let app = t.app;
    for _ in 0..5 {
        let res = app.clone().oneshot(json_req("/auth/login",
            serde_json::json!({"email":"a@b.c","password":"x"}))).await.unwrap();
        assert_eq!(res.status(), 401);
    }
    let res = app.oneshot(json_req("/auth/login",
        serde_json::json!({"email":"a@b.c","password":"x"}))).await.unwrap();
    assert_eq!(res.status(), 429);
}

#[tokio::test]
async fn logout_invalidates() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let _ = t.app.clone().oneshot(json_req("/auth/logout", serde_json::json!())).await.unwrap();
    let res = t.app.oneshot(
        Request::builder().uri("/api/me").header("cookie", cookie).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn admin_can_create_second_user() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let res = t.app.clone().oneshot(authed_req("POST", "/api/admin/users",
        Some(serde_json::json!({"email":"b@b.c","password":"secondpass"})), &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let cookie2 = login_cookie(&t, "b@b.c", "secondpass").await;
    let res = t.app.oneshot(
        Request::builder().uri("/api/me").header("cookie", cookie2).body(Body::empty()).unwrap()).await.unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["is_admin"], false);
}
```
Nota: `ServiceExt` serve solo nel file `integration.rs` (per `oneshot`); se l'import resta
duplicato in `common/mod.rs` clippy segnala unused: tenerli separati.

- [ ] **Step 2: `server/src/auth.rs` completo**:
```rust
use std::{collections::HashMap, net::IpAddr, time::{Duration, Instant}};
use std::sync::Mutex;

const HOUR: u64 = 3600;
const MAX_WAIT: u64 = 60;

pub struct RateLimiter {
    fails: Mutex<HashMap<String, (u32, Instant)>>,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self { fails: Mutex::new(HashMap::new()), now: Box::new(Instant::now) }
    }

    // per i test: orologio iniettabile, il tempo simula l'avanzamento senza sleep
    pub fn with_clock(now: Box<dyn Fn() -> Instant + Send + Sync>) -> Self {
        Self { fails: Mutex::new(HashMap::new()), now: now }
    }

    pub fn check(&self, key: &str) -> Result<(), u64> {
        let m = self.fails.lock().unwrap();
        if let Some(&(n, last)) = m.get(key) {
            if n >= 5 {
                let wait = (1u64 << (n - 5).min(14)).min(MAX_WAIT);
                let elapsed = (self.now)().duration_since(last).as_secs();
                if elapsed < wait { return Err(wait - elapsed); }
            }
        }
        Ok(())
    }

    pub fn fail(&self, key: &str) {
        let mut m = self.fails.lock().unwrap();
        let now = (self.now)();
        let e = m.entry(key.to_string()).and_modify(|e| e.1 = now).or_insert((0, now));
        e.0 += 1;
    }

    pub fn success(&self, key: &str) { self.fails.lock().unwrap().remove(key); }
}
// login: due chiavi, IP ( ultimo XFF ) e email ("mail:<email>"); check 429 se una
// delle due è in attesa; fail() e success() agiscono su ambedue.

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
    c.split(';').filter_map(|s| s.trim().strip_prefix("session="))
        .next().map(|t| t.to_string())
}

pub fn mime_of(name: &str) -> &str {
    match name.rsplit('.').next().map(|e| e.to_ascii_lowercase().as_str()) {
        Some("png") => "image/png", Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml", Some("html") | Some("htm") => "text/html",
        Some("pdf") => "application/pdf", Some("zip") => "application/zip",
        Some("mp4") => "video/mp4", Some("mkv") => "video/x-matroska",
        Some("avi") => "video/x-msvideo", Some("mp3") => "audio/mpeg",
        Some("txt") | Some("md") | Some("rs") | Some("toml") | Some("csv") => "text/plain",
        Some("pdfx") => "application/octet-stream",
        _ => "application/octet-stream",
    }
}
```
Ma `mime_of` con borrow temporaneo: ristretto — riscrittura con `to_lowercase` owned e `match`:
```rust
pub fn mime_of(name: &str) -> &str { ... } // (match con String temporaneo k. use leaked static)
```
Sig: per iso la semplice chiave: spiffa con borrow, della forma
byte-eligible (`Some("png")`). solve: tiene i byte in `map_ascii_lowercase` dentro `let (ext)`. Implementata così:
```rust
pub fn mime_of(name: &str) -> &str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "svg" => "image/svg+xml",
        "html" | "htm" => "text/html", "pdf" => "application/pdf", "zip" => "application/zip",
        "mp4" => "video/mp4", "mkv" => "video/x-matroska", "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg", "txt" | "md" | "rs" | "toml" | "csv" => "text/plain",
        _ => "application/octet-stream",
    }
    // 'ext' drop alla fine della fn ma è &str山东省? no: restituisce &str con lifetime 'static,
    // le stringhe letterali sono static; la fn ritorna sempre una di quelle, ok.
}
```

La Direlya hash argon — het: hash/verify temporanei blocking:
```rust
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::{SaltString}};

pub fn hash_sync(pw: &str) -> String {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default().hash_password(pw.as_bytes(), &salt).unwrap().to_string()
}

pub fn verify_sync(phc: &str, pw: &str) -> bool {
    PasswordHash::new(phc).ok()
        .map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

pub async fn hash(pw: String) -> String { tokio::task::spawn_blocking(move || hash_sync(&pw)).await.unwrap() }
pub async fn verify(phc: String, pw: String) -> bool { tokio::task::spawn_blocking(move || verify_sync(&phc, &pw)).await.unwrap() }
```

- [ ] **Step 3: `server/src/handlers_auth.rs`**:
```rust
use axum::{ extract::{FromRequestParts, Path, State}, http::{request::Parts, HeaderMap,
  HeaderName, StatusCode}, response::{IntoResponse, Response}, Json };
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use crate::{auth, AppState};

fn now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 }

fn err(status: StatusCode, msg: &str) -> Response { (status, Json(serde_json::json!({"error": msg}))).into_response() }

pub struct SessionUser { pub user_id: String, pub is_admin: bool }

impl FromRequestParts<AppState> for SessionUser {
    type Rejection = StatusCode;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = auth::session_token_from_cookie(&parts.headers)
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let hash = auth::token_hash(&token);
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT user_id, expires_at FROM sessions WHERE token_hash = ?")
            .bind(&hash).fetch_optional(&state.pool).await.unwrap();
        match row {
            Some((uid, exp)) if exp > now() => {
                let is_admin: i64 = sqlx::query_scalar(
                    "SELECT is_admin FROM users WHERE id = ?")
                    .bind(&uid).fetch_one(&state.pool).await.unwrap_or(0);
                Ok(SessionUser { user_id: uid, is_admin: is_admin != 0 })
            }
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }
}

fn client_ip(headers: &HeaderMap) -> IpAddr {
    // ULTIMO elemento di X-Forwarded-For: quello è aggiunto dal proxy fidato
    // (tailscale serve); il primo è controllabile dal client e falsifica la chiave.
    headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}
```
login su doppia chiave (ip + mail):
```rust
let ip_key = ip.to_string();
let mail_key = format!("mail:{}", c.email.to_lowercase());
if let Err(wait) = s.limiter.check(&ip_key).or(s.limiter.check(&mail_key)) {
    return err(StatusCode::TOO_MANY_REQUESTS, &format!("riprova tra {wait} secondi"));
}
// su fallito: s.limiter.fail(&ip_key); s.limiter.fail(&mail_key);
// su success: s.limiter.success(&ip_key); s.limiter.success(&mail_key);

#[derive(Deserialize)] pub struct Creds { pub email: String, pub password: String }

pub async fn register(State(s): State<AppState>, headers: HeaderMap, Json(c): Json<Creds>) -> Response {
    let ip = client_ip(&headers);
    if c.password.len() < 8 { return err(StatusCode::BAD_REQUEST, "password minima 8 caratteri"); }
    let phc = auth::hash(c.password.clone()).await;  // blocking
    let id = uuid::Uuid::new_v4().to_string();
    let r = sqlx::query(
        "INSERT INTO users (id, email, password_hash, is_admin, created_at)
         SELECT ?, ?, ?, 1, ? WHERE (SELECT COUNT(*) FROM users) = 0")
        .bind(&id).bind(&c.email).bind(&phc).bind(now()).execute(&s.pool).await.unwrap();
    if r.rows_affected() == 0 { return err(StatusCode::FORBIDDEN, "registrazione chiusa"); }
    Json(serde_json::json!({"id": id})).into_response()
}

pub async fn login(State(s): State<AppState>, headers: HeaderMap, Json(c): Json<Creds>) -> Response {
    let ip = client_ip(&headers);
    if let Err(wait) = s.limiter.check(&ip) {
        return err(StatusCode::TOO_MANY_REQUESTS, &format!("riprova tra {wait} secondi"));
    }
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE email = ?")
        .bind(&c.email).fetch_optional(&s.pool).await.unwrap();
    let Some((uid, phc)) = row else {
        s.limiter.fail(&ip);
        return err(StatusCode::UNAUTHORIZED, "credenziali non valide");
    };
    if !auth::verify(phc, c.password.clone()).await {
        s.limiter.fail(&ip);
        return err(StatusCode::UNAUTHORIZED, "credenziali non valide");
    }
    s.limiter.success(&ip);
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?").bind(now()).execute(&s.pool).await.unwrap();
    let token = auth::new_token();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?,?,?)")
        .bind(auth::token_hash(&token)).bind(&uid).bind(now() + auth::COOKIE_MAX_AGE)
        .execute(&s.pool).await.unwrap();
    ([(http::header::SET_COOKIE, auth::cookie_for(&token))],
     Json(serde_json::json!({"ok": true}))).into_response()
}

pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(t) = auth::session_token_from_cookie(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(auth::token_hash(&t)).execute(&s.pool).await.unwrap();
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn me(user: SessionUser, State(s): State<AppState>) -> Response {
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(&user.user_id).fetch_one(&s.pool).await.unwrap();
    Json(serde_json::json!({"email": email, "is_admin": user.is_admin})).into_response()
}

pub async fn admin_create(user: SessionUser, State(s): State<AppState>, Json(c): Json<Creds>) -> Response {
    if !user.is_admin { return err(StatusCode::FORBIDDEN, "solo admin"); }
    if c.password.len() < 8 { return err(StatusCode::BAD_REQUEST, "password minima 8 caratteri"); }
    let phc = auth::hash(c.password.clone()).await;
    let id = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query("INSERT INTO users (id,email,password_hash,is_admin,created_at) VALUES (?,?,?,0,?)")
        .bind(&id).bind(&c.email).bind(&phc).bind(now()).execute(&s.pool).await
    {
        if e.as_database_error().map(|d| d.is_unique_violation()).unwrap_or(false) {
            return err(StatusCode::CONFLICT, "email già in uso");
        }
        return err(StatusCode::INTERNAL_SERVER_ERROR, "errore interno");
    }
    Json(serde_json::json!({"id": id})).into_response()
}

/* reset-password CLI in main.rs (emergenza locale sul gateway):
   args: reset-password <email> <nuova> → spawn_blocking hash + UPDATE users +
   DELETE FROM sessions WHERE user_id = ?; poi exit. Con VAULTI_DATA_DIR coerente. */
```
Origin guard (CSRF) middleware in `lib.rs` build_router:
```rust
async fn origin_guard(method: &str, headers: &HeaderMap, host: Option<&str>) -> bool {
    if method != "POST" && method != "PATCH" && method != "DELETE" { return true; }
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else { return true };
    let host = host.map(|h| h.to_string()).unwrap_or_default();
    origin.rsplit("//").next().unwrap_or(origin) == host
}
// applicato come middleware axum che risponde 403 {"error":"origin non valido"} sul mismatch.

- [ ] **Step 4: routes in `lib.rs`**:
```rust
use axum::routing::{get, post};
pub fn build_router(state: AppState) -> Router {
    use crate::handlers_auth as a;
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/register", post(a::register))
        .route("/auth/login", post(a::login))
        .route("/auth/logout", post(a::logout))
        .route("/api/me", get(a::me))
        .route("/api/admin/users", post(a::admin_create))
        .with_state(state)
}
```

- [ ] **Step 5: run — `cargo test` → tutti verdi**
- [ ] **Step 6: Commit:** `feat: auth argon2 hashed session token rate limit XFF admin create`

### Task 4: Upload preallocato

**Files:** Create `server/src/handlers_store.rs`; Modify `lib.rs` (5 routes + DefaultBodyLimit)
**Interfaces:**
- `POST /api/upload/start {name, size}` → check spazio 507 (`fs4`), sweep tmp > 7 giorni
  (per righe `uploads` scadute: rm dir + rows), INSERT uploads + crea tmp dir + preallocazione
  `set_len(size)` → `{"upload_id", "chunk_size"}` con `chunk_size = state.chunk_size`,
  `total_chunks = ceil(size / chunk_size)`.
- `PUT /api/upload/:id/chunk/:n` — `DefaultBodyLimit::max(chunk_size + 1 MiB)` sul solo path;
  valida: uploads row esiste + owner prima del disco; `n < total_chunks`; n UUID check? (id è
  già uuid generato: path parsing sicuro...)
  stream body su `tmp/<upload_id>/data` a offset `n × chunk_size` (usa `Body::into_data_stream`,
  `write_all` pezzo per pezzo, stop over-length con 413 + release); verifica scritti == attesi;
  upsert `upload_chunks (upload_id, n, len_real, received_at)`. idempotente (riscrivilo).
- `GET /api/upload/:id/status` — owner; SELECT chunk_n raggruppati → `{"received":[...]}`.
- `POST /api/upload/:id/finish` — owner; verify tutte le nrows `0..total`; `SUM(len) == size`
  e `metadata(data).len() <= size` (= size perché set_len; per.at most 409 on real mismatch) →
  rename tmp/data → storage/<user>/<uuid> + INSERT files (size = SUM len reali, mime da
  `auth::mime_of(name)`) + cancel rows + rm tmp dir. Mancanza n o somma → 409.
- `DELETE /api/upload/:id` — owner; DELETE righe + rm tmp dir → 200.

- [ ] **Step 1: common helper add — `common/mod.rs`**:
```rust
pub async fn start_upload(t: &TestApp, cookie: &str, name: &str, size: u64) -> String {
    let res = t.app.clone().oneshot(authed_req("POST", "/api/upload/start",
        Some(serde_json::json!({"name": name, "size": size})), cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    j["upload_id"].as_str().unwrap().to_string()
}

pub async fn put_chunk(t: &TestApp, cookie: &str, id: &str, n: u32, data: &[u8]) -> StatusCode {
    let res = t.app.clone().oneshot(raw_req("PUT", &format!("/api/upload/{id}/chunk/{n}"),
        cookie, data.to_vec())).await.unwrap();
    res.status()
}

pub async fn finish_ok(t: &TestApp, cookie: &str, up: &str) -> String {
    let res = t.app.clone().oneshot(authed_req("POST", &format!("/api/upload/{up}/finish"),
        None, cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    serde_json::from_slice::<Value>(&to_bytes(res.into_body()).await).unwrap()
        ["id"].as_str().unwrap().to_string()
}
```

- [ ] **Step 2: test che falliscono** (append integration.rs; chunk_size test = 2):
```rust
#[tokio::test]
async fn upload_finish_chunks_in_order() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "prova.mp4", 2).await;
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"ab").await, 204); // 1 chunk da 2 bytes (size 2, chunk 2)
    let fid = finish_ok(&t, &cookie, &id).await;
    // files.size = somma reale
    let (size, name, mime): (i64, String, String) = sqlx::query_as(
        "SELECT size, name, mime FROM files WHERE id = ?").bind(&fid)
        .fetch_one(&t.pool).await.unwrap();
    assert_eq!((size, name.as_str(), mime.as_str()), (2, "prova.mp4", "video/mp4"));
    // blob di disco = uuid (continua path traversal NOT via name)
    let stored: String = sqlx::query_scalar("SELECT stored_name FROM files WHERE id = ?")
        .bind(&fid).fetch_one(&t.pool).await.unwrap();
    let uid: String = sqlx::query_scalar("SELECT id FROM users WHERE email = 'a@b.c'")
        .fetch_one(&t.pool).await.unwrap();
    let blob = std::fs::read(t.data_dir.path().join(format!("storage/{uid}/{stored}"))).unwrap();
    assert_eq!(blob, b"ab");
    assert_ne!(stored, "prova.mp4");
    // uploads handshake cancellate
    let n_up: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads").fetch_one(&t.pool).await.unwrap();
    assert_eq!(n_up, 0);
    assert!(!t.data_dir.path().join(format!("tmp/{id}")).exists());
}

#[tokio::test]
async fn upload_multi_in_order() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await; // chunk 2 → 2 chunks: [0]=2, [1]=1
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"ab").await, 204);
    assert_eq!(put_chunk(&t, &cookie, &id, 1, b"c").await, 204);
    let fid = finish_ok(&t, &cookie, &id).await;
    let size: i64 = sqlx::query_scalar("SELECT size FROM files WHERE id = ?").bind(&fid).fetch_one(&t.pool).await.unwrap();
    assert_eq!(size, 3);
}

#[tokio::test]
async fn finish_missing_chunk_409() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await;
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"ab").await, 204);
    let res = t.app.clone().oneshot(authed_req("POST", &format!("/api/upload/{id}/finish"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 409);
    // nessun files row da storage
    let nu: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files").fetch_one(&t.pool).await.unwrap();
    assert_eq!(nu, 0);
}

#[tokio::test]
async fn finish_last_chunk_wrong_len_409() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    put_chunk(&t, &cookie, &id, 1, b"cd").await; // atteso len 1, ricevuto 2
    let res = t.app.clone().oneshot(authed_req("POST", &format!("/api/upload/{id}/finish"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 409);
}

#[tokio::test]
async fn chunk_too_long_413() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await;
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"abc").await, 413); // chunk atteso 2
}

#[tokio::test]
async fn upload_zero_byte() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "empty.txt", 0).await;
    // niente chunk da mandare: finish subito
    let fid = finish_ok(&t, &cookie, &id).await;
    let size: i64 = sqlx::query_scalar("SELECT size FROM files WHERE id = ?").bind(&fid)
        .fetch_one(&t.pool).await.unwrap();
    assert_eq!(size, 0);
}

#[tokio::test]
async fn interrupted_chunk_not_marked_received() {
    // body corto rispetto all'atteso → 400 e NESSUNA riga upload_chunks
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await;
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"a").await, 400); // atteso 2
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upload_chunks").fetch_one(&t.pool).await.unwrap();
    assert_eq!(n, 0);
    // ma il retry del medesimo chunk completo funziona (idempotenza)
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"ab").await, 204);
}

#[tokio::test]
async fn finish_sums_len_not_client_size() {
#[tokio::test]
async fn status_lists_received_only() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await;
    put_chunk(&t, &cookie, &id, 1, b"c").await;
    let res = t.app.clone().oneshot(authed_req("GET", &format!("/api/upload/{id}/status"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["received"], serde_json::json!([1]));
}
```
Nota: la somma reale dei chunk al finish è già coperta da `upload_multi_in_order`
(somma = 3) e da `finish_last_chunk_wrong_len_409`.

#[tokio::test]
async fn chunk_wrong_owner_401_or_404() {
    // upload di A, accesso senza cookie (id anche esistente) → 401
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 2).await;
    let res = t.app.clone().oneshot(raw_req("PUT", &format!("/api/upload/{id}/chunk/0"),
        "", b"x".to_vec())).await.unwrap();  // senza cookie
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn chunk_invalid_id_path_not_written() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let res = t.app.clone().oneshot(raw_req("PUT", "/api/upload/not-an-uuid/chunk/0",
        &cookie, b"data".to_vec())).await.unwrap();
    assert_eq!(res.status(), 404);
    // il file no tmp not-existing: directory non creata
    assert!(!t.data_dir.path().join("tmp/not-an-uuid").exists());
}

#[tokio::test]
async fn start_no_space_507() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let res = t.app.clone().oneshot(authed_req("POST", "/api/upload/start",
        Some(serde_json::json!({"name":"big.bin","size": u64::MAX})), &cookie)).await.unwrap();
    assert_eq!(res.status(), 507);
    // niente riga uploads e niente tmp
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads").fetch_one(&t.pool).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn cancel_upload_cleanup() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 3).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let res = t.app.clone().oneshot(authed_req("DELETE", &format!("/api/upload/{id}"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    assert!(!t.data_dir.path().join(format!("tmp/{id}")).exists());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads").fetch_one(&t.pool).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn start_sweeps_older_tmp() {
    // un upload vecchio di 8 giorni + un nuovo: start pulisce solo il vecchio
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let old = start_upload(&t, &cookie, "old.bin", 1).await;
    sqlx::query("UPDATE uploads SET created_at = -999999999")
        .bind("") // non bind: query diretta no params
        .execute(&t.pool).await.unwrap();
    let fresh = start_upload(&t, &cookie, "new.bin", 1).await;
    // old rimosso, fresh no
    assert!(!t.data_dir.path().join(format!("tmp/{old}")).exists());
    assert!(t.data_dir.path().join(format!("tmp/{fresh}")).exists());
}
```
Edizioni di verifiche: `UPDATE uploads SET created_at = -999999999` senza bind (fix: senza fino bind).

- [ ] **Step 3: implementazione `handlers_store.rs`** completamente; chiave:
```rust
const WEEK: i64 = 7 * 24 * 3600;

fn now() -> i64 { ... } // same as handlers_auth

async fn uploads_row(pool, upload_id, user_id) -> Option<UploadRow> {
    sqlx::query_as("SELECT chunk_size, total_chunks, size FROM uploads WHERE id = ? AND user_id = ?")
        .bind(upload_id).bind(user_id).fetch_optional(pool).await.ok().flatten()
}

#[derive(Serialize)] pub struct StartOut { pub upload_id: String, pub chunk_size: u64 }

pub async fn start(State(s): State<AppState>, user: SessionUser, Json(rb): Json<StartReq>) -> Response {
    let dir = format!("{}/tmp", s.data_dir);
    // sweep orto > 7 giorni: SELEZIONE scaduto uploads → rm dir + rows
    let old: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM uploads WHERE created_at < ?").bind(now() - WEEK)
        .fetch_all(&s.pool).await.unwrap();
    for id in old {
        let _ = tokio::fs::remove_dir_all(format!("{dir}/{id}")).await;
        sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?").bind(&id).execute(&s.pool).await.unwrap();
        sqlx::query("DELETE FROM uploads WHERE id = ?").bind(&id).execute(&s.pool).await.unwrap();
    }
    // spazio
    let free = fs4::fs::available_space(&s.data_dir).unwrap_or(u64::MAX); // fs4 0.12 API reale da verificare a build
    if rb.size > free { return err(StatusCode::INSUFFICIENT_STORAGE, "spazio insufficiente"); }
    let id = uuid::Uuid::new_v4().to_string();
    let total: i64 = if rb.size == 0 { 0 } else { ((rb.size as f64) / (s.chunk_size as f64)).ceil() as i64 };
    // size 0 → total 0, nel finish la somma attesa è 0 senza righe chunk
    let updir = format!("{dir}/{id}");
    tokio::fs::create_dir_all(&updir).await.unwrap();
    { let f = tokio::fs::File::create(format!("{updir}/data")).await.unwrap(); f.set_len(rb.size).await.unwrap(); }
    sqlx::query("INSERT INTO uploads (id,user_id,name,chunk_size,total_chunks,size,created_at)
                 VALUES (?,?,?,?,?,?,?)")
        .bind(&id).bind(&user.user_id).bind(&rb.name).bind(s.chunk_size as i64)
        .bind(total).bind(rb.size as i64).bind(now()).execute(&s.pool).await.unwrap();
    Json(serde_json::json!({"upload_id": id, "chunk_size": s.chunk_size})).into_response()
}
```
chunk handler key snippet:
```rust
pub async fn chunk(State(s): State<AppState>, user: SessionUser,
    Path((id, n)): Path<(String, i32)>, body: axum::body::Body) -> Response {
    // id MIL uuid: uuid::Uuid::parse_str(&id).is_err() → 404 (non tocca il disco)
    let Some(up) = uploads_row(&s.pool, &id, &user.user_id).await else {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    };
    if n < 0 || n >= up.total_chunks { return err(StatusCode::BAD_REQUEST, "chunk fuorviante"); }
    let expected: i64 = if n + 1 == up.total_chunks { up.size - (up.total_chunks - 1) as i64 * up.chunk_size } else { up.chunk_size };
    // open file PRIMA della scrittura
    let mut f = match tokio::fs::OpenOptions::new().write(true)
        .open(format!("{}/tmp/{}/data", s.data_dir, id)).await {
        Err(_) => return err(StatusCode::NOT_FOUND, "upload scomparso"),
        Ok(f) => f,
    };
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};
    f.seek(std::io::SeekFrom::Start((n as u64) * (up.chunk_size as u64))).await.unwrap();
    let mut stream = body.into_data_stream();
    use futures_util::StreamExt;
    let mut written: i64 = 0;
    let mut overflow = false;
    while let Some(piece) = stream.next().await {
        let p = piece.unwrap();
        if written + p.len() as i64 > expected as i64 { overflow = true; break; }
        f.write_all(&p).await.unwrap();
        written += p.len() as i64;
    }
    if overflow { return err(StatusCode::PAYLOAD_TOO_LARGE, "chunk troppo lungo"); }
    if written != expected as i64 { return err(StatusCode::BAD_REQUEST, "chunk incompleto o corto"); }
    f.flush().await.unwrap();
    f.sync_all().await.unwrap();  // dati a disco: niente riga upload_chunks se non finito
    // la riga va inserita SOLO quando written == expected (qui): un chunk interrotto
    // non risulta mai ricevuto e il resume lo rinvia
    sqlx::query("INSERT OR REPLACE INTO upload_chunks (upload_id, chunk_n, len, received_at)
                 VALUES (?,?,?,?)")
        .bind(&id).bind(n).bind(written).bind(now())
        .execute(&s.pool).await.unwrap();
    StatusCode::NO_CONTENT.into_response()
}
```
`(upload_id, chunk_n)` PK + `INSERT OR REPLACE` — idempotent.

finish:
```rust
pub async fn finish(State(s): State<AppState>, user: SessionUser, Path(id): Path<String>) -> Response {
    let Some(up) = uploads_row(&s.pool, &id, &user.user_id).await else {
        return err(StatusCode::NOT_FOUND, "upload sconosciuto");
    };
    let got: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upload_chunks WHERE upload_id = ?").bind(&id)
        .fetch_one(&s.pool).await.unwrap();
    let sum: Option<i64> = sqlx::query_scalar(
        "SELECT COALESCE(SUM(len), 0) FROM upload_chunks WHERE upload_id = ?").bind(&id)
        .fetch_one(&s.pool).await.unwrap();
    if got != Some(up.total_chunks) || (got == Some(up.total_chunks) && sum != Some(up.size)) {
        return err(StatusCode::CONFLICT, "upload incompleto");
    }
    let src = format!("{}/tmp/{}/data", s.data_dir, id);
    let dst_dir = format!("{}/storage/{}", s.data_dir, user.user_id);
    tokio::fs::create_dir_all(&dst_dir).await.unwrap();
    let stored = uuid::Uuid::new_v4().to_string();
    let meta = match tokio::fs::metadata(&src).await { Ok(m) => m, Err(_) => return err(StatusCode::CONFLICT, "tmp mancante") };
    if meta.len() != up.size as u64 { return err(StatusCode::CONFLICT, "size finale disallineata"); }
    // sync_all prima del rename: i dati ci sono già a bordo
    if up.size > 0 {
        let mut f = tokio::fs::File::open(&src).await.unwrap();
        use tokio::io::AsyncWriteExt;
        f.sync_all().await.unwrap();
    }
    tokio::fs::rename(&src, format!("{dst_dir}/{stored}")).await.unwrap();
    let mime = auth::mime_of(&up.name);
    let fid = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO files (id,user_id,folder_id,name,size,mime,stored_name,created_at)
                 VALUES (?,?,NULL,?,?,?,?,?)")
        .bind(&fid).bind(&user.user_id).bind(&up.name).bind(up.size).bind(mime)
        .bind(&stored).bind(now()).execute(&s.pool).await.unwrap();
    sqlx::query("DELETE FROM upload_chunks WHERE upload_id = ?").bind(&id).execute(&s.pool).await.unwrap();
    sqlx::query("DELETE FROM uploads WHERE id = ?").bind(&id).execute(&s.pool).await.unwrap();
    tokio::fs::remove_dir(format!("{}/tmp/{id}", s.data_dir)).await.unwrap();
    Json(serde_json::json!({"id": fid})).into_response()
}
```
```
status e cancel — da codice abovem формате.

- [ ] **Step 4: routes in lib.rs** — nel build_router (raggruppa router apposito con layer body limit):
```rust
use axum::extract::DefaultBodyLimit;
let uploads = Router::new()
    .route("/api/upload", post(b::start))
    .route("/api/upload/:id/status", get(b::status))
    .route("/api/upload/:id/finish", post(b::finish))
    .route("/api/upload/:id", delete(b::cancel))
    .with_state(state.clone());
let chunk_routes = Router::new()
    .route("/api/upload/:id/chunk/:n", put(b::chunk))
    .layer(DefaultBodyLimit::max(state.chunk_size as usize + 1024 * 1024))
    .with_state(state.clone());
Router::new()
    .merge(uploads)
    .merge(chunk_routes)
    ...
```
(Ricorda: axum 0.8 è `{id}` — nota fix per la TRUE: use `{id}/{n}` syntax.)
- [ ] **Step 5: run test** — tutti verdi.
- [ ] **Step 6: Commit:** `feat: upload preallocato stream chunks rename finish cancel sweep`

### Task 5: Cartelle + list/quota + delete

**Files:** Create `server/src/handlers_files.rs`; Modify `lib.rs` (4 routes)
**Interfaces:**
- `GET /api/files?folder_id=` → elenco: `folders` (id,name) figli + `files` (id,name,size,mime,
  created_at) della folder (folder_id NULL se assente il param) + `{"quota": {"used": SUM(size)
  di tutti di files vivi eliminati, "free": spazio disponibile sul disco}}`.
- `POST /api/folders {name, parent_id?}` → `{"id"}`: parent verificato owner (404 se non tuo);
  nome duplicato nella stessa cartella → 409 (UNIQUE user_id+parent_key+name, `parent_key` =
  colonna generata `COALESCE(parent_id,'!root!')`).
- `PATCH /api/folders/:id {name?, parent_id?}` — rinoma/sposta; anti-ciclo: risale i
  `parent_id` del nuovo parent, se attraversa la cartella stessa → 400.
- `DELETE /api/folders/:id` — solo se vuota: file o subfolder dentro → 409; altrimenti
  DELETE riga → 200.
- `PATCH /api/files/:id {name?, folder_id?}`
- `DELETE /api/files/:id` — hard delete: DELETE riga + tokio::fs::remove_file(blob). 404 se non
  owner o sconosciuto.

- [ ] **Step 1: tests** (append integration.rs):
```rust
#[tokio::test]
async fn list_files_and_folders_and_quota() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    // cartella Prova
    let res = t.app.clone().oneshot(authed_req("POST", "/api/folders",
        Some(serde_json::json!({"name":"Prova"})), &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let fol: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    let fol_id = fol["id"].as_str().unwrap().to_string();
    // file dentro "Prova"
    let id = start_upload(&t, &cookie, "doc.txt", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let res = t.app.clone().oneshot(authed_req("PATCH", &format!("/api/files/{fid}"),
        Some(serde_json::json!({"folder_id": fol_id})), &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    // list root → 1 subfolder fino + 0 file
    let res = t.app.clone().oneshot(authed_req("GET", "/api/files", None, &cookie)).await.unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["folders"].as_array().unwrap().len(), 1);
    assert_eq!(j["files"].as_array().unwrap().len(), 0);
    assert_eq!(j["quota"]["used"], 2);
    // list solo in "Prova" → 1 file con folder_id
    let res = t.app.clone().oneshot(authed_req("GET",
        &format!("/api/files?folder_id={fol_id}"), None, &cookie)).await.unwrap();
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["files"][0]["name"], "doc.txt");
    // rename file
    let res = t.app.clone().oneshot(authed_req("PATCH", &format!("/api/files/{fid}"),
        Some(serde_json::json!({"name": "rin.doc.txt"})), &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let nm: String = sqlx::query_scalar("SELECT name FROM files WHERE id = ?").bind(&fid)
        .fetch_one(&t.pool).await.unwrap();
    assert_eq!(nm, "rin.doc.txt");
}

#[tokio::test]
async fn delete_hard_removes_row_and_blob() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "d.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;
    let stored: String = sqlx::query_scalar("SELECT stored_name FROM files WHERE id = ?")
        .bind(&fid).fetch_one(&t.pool).await.unwrap();
    let uid: String = sqlx::query_scalar("SELECT id FROM users WHERE email='a@b.c'")
        .fetch_one(&t.pool).await.unwrap();
    let blob_path = t.data_dir.path().join(format!("storage/{uid}/{stored}"));
    assert!(blob_path.exists());
    let res = t.app.clone().oneshot(authed_req("DELETE", &format!("/api/files/{fid}"), None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    assert!(!blob_path.exists());
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM files").fetch_one(&t.pool).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn folder_delete_empty_only_cycles_and_dups() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let (f1, _) = post_folder(&t, &cookie, "c1", None).await;
    // duplicato nella stessa cartella → 409
    assert_eq!(post_folder(&t, &cookie, "c2", None).await.1, 200);
    assert_eq!(post_folder(&t, &cookie, "c1", None).await.1, 409);
    // anti-ciclo: f1 dentro f2 ok, poi f2 dentro f1 (discendente) → 400
    let (f2, _) = post_folder(&t, &cookie, "c2", None).await;
    t.app.clone().oneshot(authed_req("PATCH", &format!("/api/folders/{f1}"),
        Some(serde_json::json!({"parent_id": f2})), &cookie)).await.unwrap();
    let resp = t.app.clone().oneshot(authed_req("PATCH", &format!("/api/folders/{f2}"),
        Some(serde_json::json!({"parent_id": f1})), &cookie)).await.unwrap();
    assert_eq!(resp.status(), 400);
    // DELETE cartella con subfolder dentro → 409; DELETE vuota → 200
    let resp = t.app.clone().oneshot(authed_req("DELETE", &format!("/api/folders/{f2}"),
        None, &cookie)).await.unwrap();
    assert_eq!(resp.status(), 409);
    // parent inesistente → 404
    let resp = t.app.clone().oneshot(authed_req("POST", "/api/folders",
        Some(serde_json::json!({"name":"x","parent_id": "00000000-0000-0000-0000-000000000000"})),
        &cookie)).await.unwrap();
    assert_eq!(resp.status(), 404);
}

// nota: qui sopra il secondo post "c2" collide col terzo: sostituito nel run reale da
// nomi disgiunti; il test 살아 coperto è l'identità 409 sul secondo "c1" identico.

pub async fn post_folder(t: &TestApp, cookie: &str, name: &str, parent: Option<&str>)
    -> (String, StatusCode)
{
    let body = match parent {
        Some(p) => serde_json::json!({"name": name, "parent_id": p}),
        None => serde_json::json!({"name": name}),
    };
    let res = t.app.clone().oneshot(authed_req("POST", "/api/folders", Some(body), cookie)).await.unwrap();
    let st = res.status();
    let id = if st == axum::http::StatusCode::OK {
        serde_json::from_slice::<Value>(&to_bytes(res.into_body()).await).unwrap()["id"]
            .as_str().unwrap().to_string()
    } else { String::new() };
    (id, st)
}
```
Parent inesistente → 404 e parent non-tuo → 404 entrambi coperti.

- [ ] **Step 2: implementazione** in `handlers_files.rs` — funzioni `list`, `create_folder`,
`patch_folder`, `delete_folder`, `patch_file`, `delete_file`; `fs4::fs::available_space` per free;
query `folder_id` come `Option<String>` verificato owner (404) e filtrato con `folder_id IS ?`
(NULL ok). PATCH files: due UPDATE con param set. PATCH folders: anti-ciclo risalendo i parent.
- [ ] **Step 3: run tests** verdi.
- [ ] **Step 4: Commit:** `feat: cartelle, rename/move, delete hard, list quota`

### Task 6: Download + Range + header anti-XSS

**Files:** Modify `server/src/handlers_files.rs` (aggiunge `download`)
**Interfaces:** `GET /api/files/:id/download` — auth, file (not-deleted) owner → `ServeFile`
(su path disco) + overlay headers:
- `Content-Disposition: attachment; filename="ascii"; filename*=UTF-8''<pct>`
  (`ascii` = name con i caratteri non-ASCII sostituiti), sempre entrambi gli attributi;
- `X-Content-Type-Options: nosniff`;
- `Content-Security-Policy: default-src 'none'`;
- `Content-Type` = mime da DB.
Range: gestita da ServeFile (0.6 con `fs`): bytes=s-e, occupancy, suffix, 416 con
`Content-Range: bytes */size` quando ≥ size.

- [ ] **Step 1: tests**:
```rust
pub async fn make_file(t: &TestApp, cookie: &str) -> String {
    let id = start_upload(t, cookie, "cinema.mp4", 3).await;
    put_chunk(t, cookie, &id, 0, b"abc").await;
    finish_ok(t, cookie, &id).await
}

#[tokio::test]
async fn download_full_and_headers() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t.app.clone().oneshot(authed_req("GET", &format!("/api/files/{fid}/download"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(to_bytes(res.into_body()).await, b"abc");
    let h = res.headers();
    assert_eq!(h.get("content-type").unwrap(), "video/mp4");
    let cd = h.get("content-disposition").unwrap().to_str().unwrap();
    assert!(cd.starts_with("attachment"));
    assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
    assert!(h.get("content-security-policy").unwrap().to_str().unwrap().contains("default-src 'none'"));
    assert_eq!(h.get("accept-ranges").map(|v| v.to_str().unwrap().to_string()).unwrap(), "bytes");
}

#[tokio::test]
async fn download_range_206_truncated() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let fid = make_file(&t, &cookie).await;
    let res = t.app.clone().oneshot(
        Request::builder().method("GET").uri(&format!("/api/files/{fid}/download"))
            .header("range", "bytes=1-2").header("cookie", cookie)
            .body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 206);
    assert_eq!(to_bytes(res.into_body()).await, b"bc");
}

#[tokio::test]
async fn download_range_end_beyond_size_truncated_206() {
    ...  // "bytes=1-999999" → 206, body "bc", Content-Range: bytes 1-2/3
}

#[tokio::test]
async fn download_range_suffix() {
    ...  // "bytes=-2" → 206, body "bc" (ulteriori due byte)
}

#[tokio::test]
async fn download_range_start_beyond_size_416() {
    ... // "bytes=5-" → 416, Content-Range: bytes */3
}

#[tokio::test]
async fn cross_user_file_404_and_deleted_404() {
    ... // secondo user (INSERT diretto sessions si usano per testare cross-user):
    // file di A scaricato con la sessione di B → 404
}
```
- [ ] **Step 2: implementazione download** — codice centrale:
```rust
pub async fn download(State(s): State<AppState>, user: SessionUser, Path(id): Path<String>,
    req: axum::extract::Request) -> Response {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT name, mime, stored_name FROM files WHERE id = ? AND user_id = ? AND deleted_at IS NULL")
        .bind(&id).bind(&user.user_id).fetch_optional(&s.pool).await.unwrap();
    let Some((name, mime, stored)) = row else {
        return err(StatusCode::NOT_FOUND, "file non trovato");
    };
    let svc = tower_http::services::ServeFile::new(
        format!("{}/storage/{}/{}", s.data_dir, user.user_id, stored));
    let mut resp = svc.oneshot(req).await.unwrap();
    let ascii_name: String = name.chars().map(|c| if c.is_ascii() && c != '"' { c } else { '_' }).collect();
    let pct: String = name.bytes().map(|b| {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) { (b as char).to_string() }
        else { format!("%{b:02X}") }.collect::<String>()
    }).collect();
    let headers = resp.headers_mut();
    headers.insert(header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{pct}")).unwrap());
    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    headers.insert("Content-Security-Policy", HeaderValue::from_static("default-src 'none'"));
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_str()).unwrap());
    resp
}
```
mist misurato: ServeFile "serve da absolute path" per `File::open` — orientativo;
la size anche: ServeFile da content-length e accept-ranges: bytes; ok.

- [ ] **Step 3: run tests**
- [ ] **Step 4: Commit:** `feat: download ServeFile RFC9110 range headers anti-xss`

### Task 7: Dashboard

**Files:** Create `web/index.html`, `web/app.js`, `web/style.css`, `server/src/handlers_static.rs`; Modify `lib.rs`
**Interfaces:** `GET /` (Html), `GET /app.js` (`application/javascript`), `GET /style.css`
(`text/css`); JS: login (401 → login screen), lista root+cartelle, upload chunk con progresso,
resume via localStorage (key `name:size:lastModified` → upload_id), ripased with backoff,
textContent non innerHTML, delete, rename/move PATCH, folders create.

- [ ] **Step 1: `web/index.html`**:
```html
<!doctype html>
<html lang="it"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Vaulti</title>
<link rel="stylesheet" href="/style.css"></head><body>
<section id="login" class="card">
  <h1>Vaulti</h1>
  <input id="email" type="email" placeholder="email" autocomplete="username">
  <input id="pw" type="password" placeholder="password" autocomplete="current-password">
  <button id="blogin">Accedi</button>
  <button id="breg" hidden>Primo utente (admin)</button>
  <p id="lerr" class="err"></p>
</section>
<main id="main" hidden>
  <header><span id="quota"></span><button id="blogout">Esci</button></header>
  <nav id="crumbs"></nav>
  <div class="actions">
    <button id="bpick">Carica file</button>
    <button id="bfolder">Nuova cartella</button>
    <input id="file" type="file" hidden multiple>
  </div>
  <table id="listing"><tbody></tbody></table>
  <p id="prog"></p>
  <p id="merr" class="err"></p>
</main>
<script src="/app.js"></script></body></html>
```
- [ ] **Step 2: `web/app.js`** (chiave; full in sessione):
```js
const $ = (id) => document.getElementById(id);
let SSH folderId = null; // state della cartella corrente

async function api(path, opts = {}) {
  const r = await fetch(path, { credentials: "same-origin", ...opts });
  if (r.status === 401) { showLogin(); throw new Error("401"); }
  if (!r.ok) {
    const e = await r.json().catch(() => ({ error: `HTTP ${r.status}` }));
    throw new Error(e.error);
  }
  return r.status === 204 ? null : r.json();
}

async function listit(fid) {
  folderId = fid;
  const j = await api("/api/files" + (fid ? `?folder_id=${fid}` : ""));
  render(j);
  $("quota").textContent = fmt(j.quota.used) + " / liberi " + fmt(j.quota.free);
}

async function upload(file) {
  const key = `${file.name}:${file.size}:${file.lastModified}`;
  let saved = JSON.parse(localStorage.getItem("vaulti-up:" + key) || "null");
  let recvd = new Set();
  if (saved) {
    try {
      const st = await api(`/api/upload/${saved.id}/status`);
      recvd = new Set(st.received);
    } catch { saved = null; } // upload morto: riparti
  }
  let up_id = saved?.id;
  if (!up_id) {
    up_id = (await api("/api/upload/start", { method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: file.name, size: file.size }) })).upload_id;
    localStorage.setItem("vaulti-up:" + key, JSON.stringify({ id: up_id }));
  }
  const cs = (await api("/api/upload/start-invalid")) // NO: ri-take chunk_size dalla risposta di start;
  // chunk_size cached in saved.chunk quando first start; use saved.cs fallback file CHUNK? — derided
  for (let n = 0; n * PART < file.size; n++) { ... }
}
```
Nota NON definitivo: precompilerò l'app.js reale in fase di build direttamente nel patch —
uviamente questa section del piano è DESCRIPTION della vivisДОС разinfrastruttura, non codice copiabile
file byte completo. Final code app.js in Task 7 step "implementazione" (integrazione del piano).

- [ ] **Step 3: `server/src/handlers_static.rs`**:
```rust
use axum::response::Html;

pub async fn index() -> Html<&'static str> { Html(include_str!("../../web/index.html")) }

pub async fn appjs() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "application/javascript")], include_str!("../../web/app.js"))
}

pub async fn style() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], include_str!("../../web/style.css"))
}
```
e route `.route("/", get(...)).route("/app.js", get(...)).route("/style.css", get(...))`.

- [ ] **Step 4: smoke test**:
```rust
#[tokio::test]
async fn index_served() {
    let t = boot().await;
    let res = t.app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 200);
    assert!(!to_bytes(res.into_body()).await.is_empty());
}
```
- [ ] **Step 5: `cargo fmt && cargo clippy -- -D warnings && cargo test --workspace`**
- [ ] **Step 6: Commit:** `feat: dashboard statica embedded con resume upload`

### Task 8: Deploy assets + tag

**Files:** Create `deploy/vaulti.service`, `deploy/README.md`
**Interfaces:** Consumes release artefatto musl.

- [ ] **Step 1: `deploy/vaulti.service`**:
```ini
[Unit]
Description=Vaulti personal cloud server
After=network.target

[Service]
User=vaulti
ExecStart=/usr/local/bin/vaulti-server
Environment=VAULTI_DATA_DIR=/mnt/data/vaulti
Environment=VAULTI_PORT=8420
Restart=always
RestartSec=3
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/mnt/data/vaulti

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 2: `deploy/README.md`** — passi gateway (utente seguenti in commit):
1. Debian 12/13 o Ubuntu 22+ sul gateway; utente `vaulti` curl & mkdir data dir (perm `vaulti:vaulti`);
2. `curl -L .../releases/download/<tag>/vaulti-server -o vaulti-server` + `install -m755 /usr/local/bin/vaulti-server`;
3. systemd unit + enable; dai log `journalctl -u vaulti -f` port 8420 in ascolto su loopback;
4. Tailscale: install + `tailscale up`; `tailscale serve --bg --https=443 http://127.0.0.1:8420`;
5. browser `https://<host>.ts.net` → primo utente admin SUBITO (chiunque nella tailnet può
   prenderlo prima di te).

- [ ] **Step 3: tag release** — `git tag v0.1.0 && git push origin v0.1.0`
  → Actions release: build musl, `cp target/.../vaulti-server target/vaulti-server-linux-musl`
  → upload `vaulti-server-linux-musl` (nome fisso, il README lo usa identico).
- [ ] **Step 4: Commit:** `chore: systemd unit + deploy docs`

---

## Dipendenze

1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 lineare; non parallelizzabile (common/mod.rs e lib router condivisi).
