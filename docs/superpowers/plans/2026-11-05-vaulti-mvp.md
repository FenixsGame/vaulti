# Vaulti MVP — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Server + dashboard web per Vaulti: cloud personale con upload a blocchi riprendibili, download con Range, auth argon2id + sessioni, servito da axum su Linux, testato e buildato via GitHub Actions.

**Architecture:** Crate unico `server` (Rust, axum 0.8, sqlx+SQLite) che espone 11 route JSON e serve la dashboard statica embedded. Blob su disco con nome UUID, metadati in SQLite. Deploy: binario da GitHub Actions + systemd + `tailscale serve`.

**Tech Stack:** Rust stable, axum 0.8, tokio, sqlx 0.8 (sqlite, runtime-tokio), argon2, uuid v4, rand, tokio-util; test con `tower::ServiceExt::oneshot` + `tempfile`.

**Spec:** `docs/superpowers/specs/2026-11-05-vaulti-mvp-design.md`

## Global Constraints

- axum `0.8` (route syntax `{param}`, non `:param`). sqlx `0.8`. Pin toolchain in `rust-toolchain.toml` (se `1.90.0` non esiste al momento dell'esecuzione, usa l'ultima stable e aggiorna la riga nel commit).
- Default listen `127.0.0.1:8420`; `VAULTI_PORT` override. Default data dir `/srv/vaulti`; `VAULTI_DATA_DIR` override (i test usano `tempfile::tempdir()`).
- `CHUNK_SIZE = 8 * 1024 * 1024` (8 MiB).
- Cookie: `format!("session={token}; HttpOnly; SameSite=Lax; Path=/; Max-Age={COOKIE_MAX_AGE}")` con `COOKIE_MAX_AGE = 30 * 24 * 3600`. (Spec prevede flag Secure nel proxy di produzione: `tailscale serve` forza HTTPS quindi il flag Secure va aggiunto come `Secure;` in produzione — decisione runtime: aggiungiamo già `Secure;` alla stringa.)
- Token sessione: 32 byte random → 64 chars esadecimali.
- Errori JSON: `{"error":"<msg>"}`.
- Ogni test autenticato manda header `cookie: session=<token>`.
- Ogni task: test fallito → minimo → test verde → commit conventional.

## Review Focus (spec implica, test fissano)

1. **Range end oltre size → 416** con `Content-Range: bytes */<size>` (Task 5).
2. **Cross-user → 404**: l'id di un file di un altro utente non scaricabile (Task 5).
3. **Sessione scaduta → 401** (Task 3).
4. **Finish con chunk mancanti → 409**, nessun blob scritto su disco (Task 4).
5. **Path traversal**: il `name` del client è solo metadato; su disco è UUID del server (Task 4 test lo verifica).

---

## File Structure (bersaglio finale)

```
Cargo.toml, rust-toolchain.toml, .gitignore
.github/workflows/ci.yml
server/Cargo.toml
server/src/lib.rs             ← moduli, AppState, build_router
server/src/main.rs            #[tokio::main] → lib::run()
server/src/config.rs          ← env load
server/src/db.rs              ← pool sqlite + migrate
server/src/migrations.sql     ← 4 tabelle
server/src/auth.rs            ← argon2, token, cookie parse, RateLimiter, SessionUser
server/src/handlers_auth.rs   ← register/login/logout/me
server/src/handlers_store.rs  ← upload start/chunk/status/finish
server/src/handlers_files.rs  ← list, download Range, delete
server/src/handlers_static.rs ← web embedded
server/tests/common/mod.rs    ← TestApp + helpers comuni dei test
server/tests/integration.rs   ← tutti i test di integrazione
web/index.html, web/app.js, web/style.css
deploy/vaulti.service, deploy/README.md
```

---

### Task 1: Scaffold workspace + CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `server/Cargo.toml`, `server/src/lib.rs`, `server/src/main.rs`, `server/src/config.rs`, `.github/workflows/ci.yml`

**Interfaces:**
- Produces: binario con `GET /healthz` → `200 "ok"`; `vaulti_server::run()`; `config::Config { data_dir: String, port: u16 }`.

- [ ] **Step 1: file di build**

`Cargo.toml` (root):
```toml
[workspace]
resolver = "2"
members = ["server"]
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "1.90.0"
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
serde_urlencoded = "0.7"

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
tempfile = "3"
```

- [ ] **Step 2: server minimo con lib**

`server/src/config.rs`:
```rust
use std::env;

#[derive(Clone)]
pub struct Config {
    pub data_dir: String,
    pub port: u16,
}

pub fn load() -> Config {
    Config {
        data_dir: env::var("VAULTI_DATA_DIR").unwrap_or_else(|_| "/srv/vaulti".into()),
        port: env::var("VAULTI_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8420),
    }
}
```

`server/src/lib.rs`:
```rust
pub mod config;

pub fn run() { /* Task 2 sostituisce con serve reale; Task 7 completa */ }
```

`server/src/main.rs`:
```rust
fn main() { vaulti_server::run() }
```

- [ ] **Step 3: CI** — `.github/workflows/ci.yml`:
```yaml
name: ci
on:
  push: { branches: [main] }
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: "clippy, rustfmt" }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy -D warnings
      - run: cargo test --workspace
      - uses: actions/upload-artifact@v4
        with: { name: vaulti-linux, path: target/release/vaulti-server }
      - run: cargo build --release
        if: startsWith(github.ref, 'refs/tags/v')
  release:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: test
    runs-on: ubuntu-latest
    permissions: { contents: write }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release
      - uses: softprops/action-gh-release@v2
        with:
          files: target/release/vaulti-server
```

- [ ] **Step 4: Commit**
```bash
git add -A && git commit -m "chore: workspace scaffold"
```

### Task 2: DB + migrazioni + helper test

**Files:**
- Modify: `server/Cargo.toml` (add deps)
- Create: `server/src/db.rs`, `server/src/migrations.sql`, `server/tests/common/mod.rs`
- Modify: `server/src/lib.rs` (AppState + build_router vuoto con /healthz)

**Interfaces:**
- Produces: `db::new_pool(path) -> SqlitePool` (":memory:" → pool 1 conn; file → pool 5, foreign_keys on), `db::migrate(&pool)` crea `users, sessions, files, upload_chunks`; `AppState::new(pool, data_dir)`; `build_router(AppState) -> Router`; `TestApp::boot() -> TestApp{ app, pool, data_dir }` + helper `json_req`, `authed_req`, `register_user`, `login_cookie`, `to_bytes`.

- [ ] **Step 1: deps**

In `server/Cargo.toml` `[dependencies]` aggiungi:
```toml
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "migrate"] }
uuid = { version = "1", features = ["v4"] }
rand = "0.8"
argon2 = "1"
tokio-util = { version = "0.7", features = ["io"] }
fs4 = "0.12"
```
Mantieni i dev-deps del Task 1.

- [ ] **Step 2: migration SQL** — `server/src/migrations.sql`:
```sql
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  is_admin INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE sessions (
  token TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  expires_at INTEGER NOT NULL
);
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  size INTEGER NOT NULL,
  mime TEXT NOT NULL,
  stored_name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE TABLE upload_chunks (
  upload_id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  chunk_n INTEGER NOT NULL,
  total_chunks INTEGER NOT NULL,
  total_size INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (upload_id, chunk_n)
);
CREATE INDEX idx_files_user ON files(user_id, deleted_at);
```

- [ ] **Step 3: `server/src/db.rs`**
```rust
use sqlx::{sqlite::{SqliteConnectOptions, SqlitePoolOptions}, SqlitePool};

pub async fn new_pool(path: &str) -> sqlx::Result<SqlitePool> {
    let mut opts = SqliteConnectOptions::new()
        .create_if_missing(true)
        .foreign_keys(true);
    opts = match path {
        ":memory:" => opts.shared_cache(true),
        p => opts.filename(p),
    };
    let max = if path == ":memory:" { 1 } else { 5 };
    SqlitePoolOptions::new().max_connections(max).connect_with(opts).await
}

pub async fn migrate(p: &SqlitePool) -> sqlx::Result<()> {
    sqlx::migrate!("src/migrations.sql").run(p).await
}
```

- [ ] **Step 4: AppState + router in `lib.rs`** (aggiungi in fondo, `pub mod` dei moduli che esistono solo più avanti non serve):
```rust
use axum::{routing::get, Router};
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub data_dir: Arc<String>,
    pub limiter: Arc<crate::auth::RateLimiter>,
}

impl AppState {
    pub fn new(pool: SqlitePool, data_dir: String) -> Self {
        Self { pool, data_dir: Arc::new(data_dir), limiter: Arc::new(crate::auth::RateLimiter::new()) }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new().route("/healthz", get(|| async { "ok" })).with_state(state)
}

pub fn run() {
    let cfg = config::load();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let pool = db::new_pool(&cfg.data_dir).await.unwrap();
        // shortcut: fino a Task 7 data_dir è anche path db; successivamente db = {data_dir}/vaulti.db
        db::migrate(&pool).await.unwrap();
        let state = AppState::new(pool, cfg.data_dir.clone());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", cfg.port)).await.unwrap();
        println!("vaulti ascolta su 127.0.0.1:{}", cfg.port);
        axum::serve(listener, build_router(state)).await.unwrap();
    });
}
```
**Nota Task 2:** `run()` usa `RateLimiter` definito nel Task 3. Al Task 2 compili con lo stub minimo in `auth.rs` (struct + `new()`), il Task 3 aggiunge i metodi. Stub:
```rust
use std::{collections::HashMap, net::IpAddr, time::Instant};
use std::sync::Mutex;

pub struct RateLimiter { fails: Mutex<HashMap<IpAddr, (u32, Instant)>> }
impl RateLimiter {
    pub fn new() -> Self { Self { fails: Mutex::new(HashMap::new()) } }
}
```
Task 3 completa i metodi; cambiamenti testati.

- [ ] **Step 5: test helper — `server/tests/common/mod.rs`**:
```rust
use axum::http::Request;
use axum::body::Body;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower::ServiceExt;
use std::path::PathBuf;

pub struct TestApp {
    pub app: axum::Router,
    pub pool: SqlitePool,
    pub data_dir: tempfile::TempDir,
}

pub async fn boot() -> TestApp {
    let data_dir = tempfile::tempdir().unwrap();
    let pool = vaulti_server::db::new_pool(":memory:").await.unwrap();
    vaulti_server::db::migrate(&pool).await.unwrap();
    let state = vaulti_server::AppState::new(pool.clone(), data_dir.path().to_string_lossy().to_string());
    TestApp { app: vaulti_server::build_router(state), pool, data_dir }
}

pub fn json_req(uri: &str, body: Value) -> Request<Body> {
    Request::builder().method("POST").uri(uri)
        .header("content-type", "application/json")
        .header("x-forwarded-for", "1.2.3.4")
        .body(Body::from(body.to_string())).unwrap()
}

pub fn authed_req(method: &str, uri: &str, body: Option<Value>, cookie: &str) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(uri)
        .header("cookie", cookie.to_string())
        .header("x-forwarded-for", "1.2.3.4");
    if let Some(v) = body {
        b = b.header("content-type", "application/json");
        b.body(Body::from(v.to_string())).unwrap()
    } else {
        b.body(Body::empty()).unwrap()
    }
}

pub async fn to_bytes(b: Body) -> Vec<u8> {
    b.collect().await.unwrap().to_bytes().to_vec()
}

pub async fn register_user(t: &TestApp, email: &str, pw: &str) {
    let app = t.app.clone();
    let res = app.oneshot(json_req("/auth/register", serde_json::json!({"email":email,"password":pw}))).await.unwrap();
    assert_eq!(res.status(), 200);
}

pub async fn login_cookie(t: &TestApp, email: &str, pw: &str) -> String {
    let app = t.app.clone();
    let res = app.oneshot(json_req("/auth/login", serde_json::json!({"email":email,"password":pw}))).await.unwrap();
    assert_eq!(res.status(), 200);
    let c = res.headers().get("set-cookie").unwrap().to_str().unwrap();
    c.split(';').next().unwrap().to_string()
}
```

- [ ] **Step 6: test migrazione — `server/tests/integration.rs`**:
```rust
mod common;
use common::*;

#[tokio::test]
async fn migrations_create_tables() {
    let t = boot().await;
    for table in ["users", "sessions", "files", "upload_chunks"] {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?")
            .bind(table).fetch_one(&t.pool).await.unwrap();
        assert_eq!(n, 1, "tabella {table} esistente");
    }
}
```
E sposta l'eventuale `#[cfg(test)] mod tests` del Task 1 in `server/tests/integration.rs` così (test healthz:
```rust
#[tokio::test]
async fn healthz_ok() {
    let t = boot().await;
    let res = t.app.clone().oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 200);
}
```

- [ ] **Step 7: run** — `cargo test --workspace` → PASS (2 test).
- [ ] **Step 8: Commit** — `feat: sqlite, migrazioni, harness test`

### Task 3: Auth completa

**Files:**
- Modify: `server/src/auth.rs` (RateLimiter completa + token + cookie + hash)
- Create: `server/src/handlers_auth.rs`
- Modify: `lib.rs` (routes), `common/mod.rs` (niente)
- Test: `server/tests/integration.rs`
**Interfaces:**
- Produces: route `POST /auth/register|login|logout`, `GET /api/me`, `SessionUser` extractor: singolo campo `user_id: String` + `is_admin: bool`.
- RateLimiter: `check(ip)->Result<_, u64>` `fail(ip)` `success(ip)` — spec: dopo 5 falliti in 1h → attesa `2^(n-5)` sec, clamp a 60.
- Token: `new_token() -> String` (64 cfr hex via rand::random [u8;32]).
- `hash(password) -> String` argon2 PHC; `verify(phc, pw) -> bool`.

- [ ] **Step 1: test che falliscono** (append a `integration.rs`):

```rust
#[tokio::test]
async fn register_first_user_then_403() {
    let t = boot().await;
    let app = t.app.clone();
    let res = app.clone().oneshot(json_req("/auth/register",
        serde_json::json!({"email":"a@b.c","password":"pw"}))).await.unwrap();
    assert_eq!(res.status(), 200);
    let res = app.oneshot(json_req("/auth/register",
        serde_json::json!({"email":"d@e.f","password":"pw2"}))).await.unwrap();
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn login_ok_cookie_and_me() {
    let t = boot().await;
    register_user(&t, "a@b.c", "pw").await;
    let cookie = login_cookie(&t, "a@b.c", "pw").await;
    assert!(cookie.starts_with("session=") && cookie.len() > "session=".len() + 30);

    let res = t.app.clone().oneshot(
        axum::http::Request::builder().uri("/api/me")
            .header("cookie", cookie).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 200);
    let json: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(json["email"], "a@b.c");
    assert_eq!(json["is_admin"], 1);
}

#[tokio::test]
async fn login_bad_password_401() {
    let t = boot().await;
    register_user(&t, "a@b.c", "pw").await;
    let res = t.app.clone().oneshot(json_req("/auth/login",
        serde_json::json!({"email":"a@b.c","password":"wrong"}))).await.unwrap();
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
    register_user(&t, "a@b.c", "pw").await;
    let cookie = login_cookie(&t, "a@b.c", "pw").await;
    sqlx::query("UPDATE sessions SET expires_at = -1").execute(&t.pool).await.unwrap();
    let res = t.app.oneshot(authed_req("GET", "/api/me", None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 401);
}

#[tokio::test]
async fn login_ratelimit_429() {
    let t = boot().await;
    register_user(&t, "a@b.c", "pw").await;
    let app = t.app.clone();
    for _ in 0..5 {
        let res = app.clone().oneshot(json_req("/auth/login",
            serde_json::json!({"email":"a@b.c","password":"x"}))).await.unwrap();
        assert_eq!(res.status(), 401);
    }
    let res = app.oneshot(json_req("/auth/login",
        serde_json::json!({"email":"a@b.c","password":"x"}))).await.unwrap();
    assert_eq!(res.status(), 429);
}
```

- [ ] **Step 2: `server/src/auth.rs`** — completa:
```rust
use std::{collections::HashMap, net::IpAddr, time::{Duration, Instant}};
use std::sync::Mutex;

const MAX_WAIT: u64 = 60;

pub struct RateLimiter { fails: Mutex<HashMap<IpAddr, (u32, Instant)>> }
impl RateLimiter {
    pub fn new() -> Self { Self { fails: Mutex::new(HashMap::new()) } }

    pub fn check(&self, ip: &IpAddr) -> Result<(), u64> {
        let mut m = self.fails.lock().unwrap();
        if let Some(&(n, t0)) = m.get(ip) {
            if n >= 5 {
                let wait = 1u64 << (n.saturating_sub(5)).min(7);
                let elapsed = t0.elapsed().as_secs();
                if elapsed < wait {
                    return Err((wait - elapsed).min(MAX_WAIT));
                }
            }
            if t0.elapsed() > Duration::from_secs(3600) { m.remove(ip); }
        }
        Ok(())
    }
    pub fn fail(&self, ip: &IpAddr) {
        let mut m = self.fails.lock().unwrap();
        let e = m.entry(*ip).or_insert((0, Instant::now()));
        e.0 += 1;
    }
    pub fn success(&self, ip: &IpAddr) { self.fails.lock().unwrap().remove(ip); }
}

pub const COOKIE_MAX_AGE: i64 = 30 * 24 * 3600;

pub fn new_token() -> String {
    use rand::Rng;
    let b: [u8; 32] = rand::random();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn cookie_for(token: &str) -> String {
    format!("session={token}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={COOKIE_MAX_AGE}")
}

pub fn session_token_from_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    headers.get("cookie")?.to_str().ok()?.split(';').find_map(|c| {
        let c = c.trim();
        c.starts_with("session=").then(|| c.trim_start_matches("session=").to_string())
    })
}

pub fn hasher() -> argon2::Argon2<'static> { argon2::Argon2::default() }

pub fn hash(pw: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    hasher().hash_password(pw.as_bytes(), &salt).unwrap().to_string()
}

pub fn verify(phc: &str, pw: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    let h = PasswordHash::new(phc).unwrap();
    hasher().verify_password(pw.as_bytes(), &h).is_ok()
}
```

- [ ] **Step 3: `server/src/handlers_auth.rs`** — 4 handler + `SessionUser` FromRequestParts chiavi:
```rust
use axum::{extract::{State, FromRequestParts}, http::request::Parts, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use crate::{auth, AppState};

fn now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 }

pub struct SessionUser { pub user_id: String, pub is_admin: bool }

impl axum::extract::FromRequestParts<AppState> for SessionUser {
    type Rejection = axum::http::StatusCode;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Some(token) = auth::session_token_from_cookie(&parts.headers) else {
            return Err(axum::http::StatusCode::UNAUTHORIZED);
        };
        let row: Option<(String, i64)> = sqlx::query_as(
            "SELECT user_id, expires_at FROM sessions WHERE token = ?")
            .bind(&token).fetch_optional(&state.pool).await.unwrap();
        match row {
            Some((uid, exp)) if exp > now() => {
                let is_admin: bool = sqlx::query_scalar(
                    "SELECT is_admin FROM users WHERE id = ?")
                    .bind(&uid).fetch_one(&state.pool).await.unwrap_or(0) != 0;
                Ok(SessionUser { user_id: uid, is_admin })
            }
            _ => Err(axum::http::StatusCode::UNAUTHORIZED),
        }
    }
}

fn err(status: u16, msg: &str) -> Response { (status, Json(serde_json::json!({"error": msg}))).into_response() }

#[derive(Deserialize)] pub struct Creds { email: String, password: String }

pub async fn register(State(s): State<AppState>, Json(c): Json<Creds>) -> Response {
    let n_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&s.pool).await.unwrap();
    if n_users > 0 { return err(403, "registrazione chiusa"); }
    let id = uuid::Uuid::new_v4().to_string();
    let phc = auth::hash(&c.password);
    sqlx::query("INSERT INTO users (id,email,password_hash,is_admin,created_at) VALUES (?,?,?,1,?)")
        .bind(&id).bind(&c.email).bind(&phc).bind(now()).execute(&s.pool).await.unwrap();
    Json(serde_json::json!({"id": id})).into_response()
}

pub async fn login(State(s): State<AppState>, headers: axum::http::HeaderMap, Json(c): Json<Creds>) -> Response {
    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    if let Err(wait) = s.limiter.check(&ip) {
        return err(429, &format!("riprova tra {wait} secondi"));
    }

    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE email = ?")
        .bind(&c.email).fetch_optional(&s.pool).await.unwrap();
    let Some((uid, phc)) = row else {
        s.limiter.fail(&ip);
        return err(401, "credenziali non valide");
    };
    if !auth::verify(&phc, &c.password) {
        s.limiter.fail(&ip);
        return err(401, "credenziali non valide");
    }

    s.limiter.success(&ip);
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?").bind(now()).execute(&s.pool).await.unwrap();
    let token = auth::new_token();
    sqlx::query("INSERT INTO sessions (token,user_id,expires_at) VALUES (?,?,?)")
        .bind(&token).bind(&uid).bind(now() + auth::COOKIE_MAX_AGE)
        .execute(&s.pool).await.unwrap();

    ([(axum::http::header::SET_COOKIE, auth::cookie_for(&token))],
     Json(serde_json::json!({"ok": true}))).into_response()
}

pub async fn logout(headers: axum::http::HeaderMap, State(s): State<AppState>) -> Response {
    if let Some(token) = auth::session_token_from_cookie(&headers) {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(token).execute(&s.pool).await.unwrap();
    }
    Json(serde_json::json!({"ok": true})).into_response()
}

- [ ] **Step 4: routes in lib.rs** — sostituisci `build_router`:
```rust
use axum::routing::{get, post};
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/auth/register", post(handlers_auth::register))
        .route("/auth/login", post(handlers_auth::login))
        .route("/auth/logout", post(handlers_auth::logout))
        .route("/api/me", get(handlers_auth::me))
        .with_state(state)
}
```
`me` handler:
```rust
pub async fn me(user: SessionUser, State(s): State<AppState>) -> Response {
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = ?")
        .bind(&user.user_id).fetch_one(&s.pool).await.unwrap();
    Json(serde_json::json!({"email": email, "is_admin": user.is_admin})).into_response()
}
```

- [ ] **Step 5: run** — `cargo test --workspace` → 7 test PASS.
- [ ] **Step 6: Commit** — `feat: argon2 auth, sessioni cookie, rate limit login`

### Task 4: Upload a blocchi

**Files:**
- Create: `server/src/handlers_store.rs`
- Modify: `lib.rs` (4 routes + body limit), `common/mod.rs` (helpers upload)
- Test: `integration.rs`
**Interfaces:**
- Produces const `CHUNK_SIZE: i64 = 8 * 1024 * 1024`.
- Route: `POST /api/upload/start {name,size}` → `{"upload_id", "chunk_size"}` (InsertOrReplace una riga in `upload_chunks` con `chunk_n=-1` che marca "perso" o placeholder; **design)**: riga upload_chunks ha chunk_n per chunk e una riga per la meta:
  Ulteriore design — righe `upload_chunks`: (upload_id, user_id, name, chunk_n, total_chunks, total_size). Metadata row `chunk_n=-1` marca il task upload (nome/total). Chunk rows chunk_n>=0.
- `PUT /api/upload/{id}/chunk/{n}` body bytes → tmp file `tmp/<upload_id>/part_<n>` + `INSERT OR REPLACE` row. Content-type raw. Limit body: `CHUNK_SIZE + 1 MiB` shipped advised.
- `GET /api/upload/{id}/status` → `{"received":[n,...]}` (chunk_n >= 0)
- `POST /api/upload/{id}/finish` → verify segno 0..total-1 FULL LIST, concatenare tmp in `storage/<user_id>/<uuid>`, insert `files` row, delete chunk rows + tmp files, `{"id": "<fileid>"}`. Mancanza → 409.

- [ ] **Step 1: common helpers — `common/mod.rs` append:**
```rust
pub async fn put_chunk(t: &TestApp, cookie: &str, id: &str, n: u32, data: &[u8]) -> axum::http::StatusCode {
    let res = t.app.clone().oneshot(
        Request::builder().method("PUT")
            .uri(&format!("/api/upload/{id}/chunk/{n}"))
            .header("cookie", cookie)
            .body(Body::from(data.to_vec())).unwrap()).await.unwrap();
    res.status()
}

pub async fn start_upload(t: &TestApp, cookie: &str, name: &str, size: u64) -> String {
    let res = t.app.clone().oneshot(authed_req("POST", "/api/upload/start",
        Some(serde_json::json!({"name": name, "size": size})), cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    j["upload_id"].as_str().unwrap().to_string()
}

pub async fn logged_in(t: &TestApp) -> String {
    register_user(t, "a@b.c", "pw").await;
    login_cookie(t, "a@b.c", "pw").await
}
```

- [ ] **Step 2: test che falliscono** (append):
```rust
#[tokio::test]
async fn upload_finish_concat_in_order() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "video.mp4", 2).await;
    assert_eq!(put_chunk(&t, &cookie, &id, 0, b"a").await, 204);
    assert_eq!(put_chunk(&t, &cookie, &id, 1, b"b").await, 204);
    let res = t.app.clone().oneshot(authed_req("POST", &format!("/api/upload/{id}/finish"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let used: i64 = sqlx::query_scalar("SELECT SUM(size) FROM files").fetch_one(&t.pool).await.unwrap();
    assert_eq!(used, 2);
    let stored_name: String = sqlx::query_scalar("SELECT stored_name FROM files").fetch_one(&t.pool).await.unwrap();
    assert_ne!(stored_name, "video.mp4"); // su disco tutto uuid
    let blob = std::fs::read(t.data_dir.path().join(format!("storage/main/{stored_name}"))).unwrap();
    assert_eq!(&blob, b"ab");
}

#[tokio::test]
async fn finish_missing_chunk_409() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "x.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"a").await;
    let res = t.app.clone().oneshot(authed_req("POST", &format!("/api/upload/{id}/finish"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 409);
    assert!(!t.data_dir.path().join("storage").join("main").exists());
}

#[tokio::test]
async fn status_lists_received_only() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "x.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"a").await;
    let res = t.app.clone().oneshot(authed_req("GET", &format!("/api/upload/{id}/status"),
        None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["received"], serde_json::json!([0]));
}

#[tokio::test]
async fn chunk_without_session_401() {
    let t = boot().await;
    register_user(&t, "a@b.c", "pw").await; // utenti esistono, ma la sessione startup: niente cookie
    let res = t.app.clone().oneshot(
        Request::builder().method("PUT").uri("/api/upload/u1/chunk/0")
            .body(Body::from("data")).unwrap()).await.unwrap();
    assert_eq!(res.status(), 401);
}
```
**Nota id `storage/mock_user/`:** in `upload_finish_concat_in_order` il blob path è `storage/<user_id>/<stored_name>`; il test legge through `SELECT stored_name` e join `storage/<user_id>/` — recupera il UUID dell'user con `SELECT id FROM users` in test (vedi codice finale reale).

- [ ] **Step 3: implementazione `handlers_store.rs`** — chiave:
- `CHUNK_SIZE: i64`; body limits: `DefaultBodyLimit::max(CHUNK_SIZE as usize + 1024*1024)` per `chunk` route singola NOT globale (altro route / vento full JSON).
- `start`: DROP write file dir; insert meta row (`chunk_n=-1`): `INSERT INTO upload_chunks (upload_id,user_id,name,chunk_n,total_chunks,total_size,updated_at) VALUES (?,?,?,?,?,?,?)` con `total_chunks = ceil(size/CHUNK_SIZE)`.
- `chunk`: bytes Bytes → `tokio::fs::write(tmp_path(id, n), bytes)`, `INSERT OR REPLACE`.
- `status`: SELECT chunk_n WHERE upload_id AND chunk_n >= 0 → sorted vec.
- `finish`: load meta row; collect received; check `vec == 0..total`; se manca → 409. Altrimenti: apri dest `tokio::fs::File::create(storage/<user>/<uuid>)`; per `n` ascendente `std::io::Read` append (`AsyncReadExt + seek`); `delete` part files; DELETE rows; INSERT `files` (id=uuid, name, size, mime da ext dalla `name`, stored_name=uuid).
- Multipart non: body `Bytes`.

- [ ] **Step 4: run** — `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** — `feat: upload a blocchi in 4 punti con resume`

### Task 5: List/quota + download con Range + delete

**Files:**
- Create: `server/src/handlers_files.rs`
- Modify: `lib.rs`
- Test: `integration.rs`
**Interfaces:**
- Produces route `GET /api/files` → `{"files":[{...}] ,"quota":{"used":n,"free":n}}`; `GET /api/files/{id}/download` con Range; `DELETE /api/files/{id}` soft-delete (togliente `deleted_at`).
- Mime da ext via piccola mappa (framework: "mp4/mkv/mp3/pdf/jpg/png/zip/text") default `application/octet-stream`.

- [ ] **Step 1: test che falliscono:**
```rust
#[tokio::test]
async fn files_list_and_delete() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "nota.txt", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid: String = finish_ok(&t, &cookie, &id).await; // helper → {"id"} value

    let res = t.app.clone().oneshot(authed_req("GET", "/api/files", None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    assert_eq!(j["files"][0]["name"], "nota.txt");
    assert_eq!(j["files"][0]["size"], 2);

    let res = t.app.clone().oneshot(authed_req("DELETE", &format!("/api/files/{fid}"), None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let del: Option<i64> = sqlx::query_scalar("SELECT deleted_at FROM files WHERE id = ?")
        .bind(&fid).fetch_one(&t.pool).await.unwrap();
    assert!(del.is_some());

    // download di file soft-delete → 404
    let res = t.app.clone().oneshot(authed_req("GET", &format!("/api/files/{fid}/download"), None, &cookie)).await.unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn download_range_full_206_416() {
    let t = boot().await;
    let cookie = logged_in(&t).await;
    let id = start_upload(&t, &cookie, "a.bin", 2).await;
    put_chunk(&t, &cookie, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &cookie, &id).await;

    let dl = |rng: Option<&str>| {
        let mut b = Request::builder().method("GET")
            .uri(&format!("/api/files/{fid}/download")).header("cookie", cookie);
        if let Some(r) = rng { b = b.header("range", r); }
        b.body(Body::empty()).unwrap()
    };

    let res = t.app.clone().oneshot(dl(None)).await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(to_bytes(res.into_body()).await, b"ab");

    let res = t.app.clone().oneshot(dl(Some("bytes=1-"))).await.unwrap();
    assert_eq!(res.status(), 206);
    let h = res.headers();
    assert_eq!(h.get("content-range").unwrap(), "bytes 1-1/2");
    assert_eq!(to_bytes(res.into_body()).await, b"b");

    let res = t.app.clone().oneshot(dl(Some("bytes=5-"))).await.unwrap();
    assert_eq!(res.status(), 416);
    assert_eq!(res.headers().get("content-range").unwrap(), "bytes */2");
}

#[tokio::test]
async fn cross_user_404() {
    let t = boot().await;
    let c1 = logged_in(&t).await;
    let id = start_upload(&t, &c1, "a.bin", 2).await;
    put_chunk(&t, &c1, &id, 0, b"ab").await;
    let fid = finish_ok(&t, &c1, &id).await;

    // secondo utente non possibile via register (chiusa): test crea user al DB diretto
    sqlx::query("INSERT INTO users (id,email,password_hash,is_admin,created_at) VALUES ('u2','b@b.c','x',0,0)")
        .execute(&t.pool).await.unwrap();
    sqlx::query("INSERT INTO sessions (token,user_id,expires_at) VALUES ('tok2','u2',99999999999)")
        .execute(&t.pool).await.unwrap();
    let res = t.app.clone().oneshot(authed_req("GET", &format!("/api/files/{fid}/download"),
        None, "session=tok2")).await.unwrap();
    assert_eq!(res.status(), 404);
}
```
Helper `finish_ok`:
```rust
pub async fn finish_ok(t: &TestApp, cookie: &str, up: &str) -> String {
    let res = t.app.clone().oneshot(authed_req("POST", &format!("/api/upload/{up}/finish"),
        None, cookie)).await.unwrap();
    assert_eq!(res.status(), 200);
    let j: Value = serde_json::from_slice(&to_bytes(res.into_body()).await).unwrap();
    j["id"].as_str().unwrap().to_string()
}
```
- [ ] **Step 2: implementazione `handlers_files.rs`**:
- `list`: SELECT id,name,size,mime,created_at WHERE user_id AND deleted_at IS NULL; used = COALESCE(SUM(size)); `free = fs4::fs::available_space(data_dir)`.
- `download`: trova per id+user (deleted_at null → else 404); apri File; Content-Length size; headers `Accept-Ranges: bytes`; parse range `bytes=<s>-<e?>`; `seek(s)` + `ReaderStream::with_capacity(64 * 1024)`: reply 206 with Content-Range; end-range → 416 con Content-Range `bytes */<size>`.
- `delete`: UPDATE `deleted_at = now` (solo per user; id non trovato → 404).
- [ ] **Step 3: run + clippy**
- [ ] **Step 4: Commit** — `feat: files list, download Range, delete`

### Task 6: Dashboard statica

**Files:**
- Create: `web/index.html`, `web/style.css`, `web/app.js`, `server/src/handlers_static.rs`
- Modify: `lib.rs` (3 routes) — include_str! embedded.
**Interfaces:**
- Produces: `GET /` html, `GET /app.js`, `GET /style.css` con Content-Type esatti. JS chiama le API così‑come‑fatto nei task precedenti.

- [ ] **Step 1: `web/index.html`** — shell minima Italiano:
```html
<!doctype html>
<html lang="it">
<head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Vaulti</title>
<link rel="stylesheet" href="/style.css">
</head>
<body>
<div id="login" class="card">
  <h1>Vaulti</h1>
  <input id="email" type="email" placeholder="email">
  <input id="pw" type="password" placeholder="password">
  <button id="blogin" type="button">Accedi</button>
  <button id="breg" type="button">Primo utente (admin)</button>
</div>
<div id="main" class="card" hidden>
  <button id="blogout" type="button">Esci</button>
  <span id="quota"></span>
  <button id="bpick" type="button">Carica file</button>
  <input id="file" type="file" hidden multiple>
  <table id="filelist"></table>
  <div id="prog"></div>
</div>
<script src="/app.js"></script>
</body>
</html>
```
- [ ] **Step 2: `web/app.js`** — funzioni fetch API corrette con take `cookie=`; uso endpoint:
```js
async function api(path, opts={}) {
  const r = await fetch(path, {credentials: 'same-origin', ...opts});
  if (r.status === 401) { show('login'); throw new Error('401'); }
  if (!r.ok) throw new Error((await r.json()).error);
  return r.json();
}
async function upload(file) {
  const {upload_id: id, chunk_size} = await api('/api/upload/start', {method:'POST',
      headers:{'content-type':'application/json'}, body: JSON.stringify({name:file.name, size:file.size})});
  // loop
  let rec = new Set();
  try {
    const st = await api(`/api/upload/${id}/status`); rec = new Set(st.received);
  } catch {
    // shortcut: nuovo upload se status fallisce — l'endpoint esiste sempre al ciclo attuale
  }
  for (let n = 0; n*chunk_size < file.size; n++) {
    if (rec.has(n)) continue;
    const part = file.slice(n*chunk_size, (n+1)*chunk_size);
    const c = await fetch(`/api/upload/${id}/chunk/${n}`, {method:'PUT', credentials:'same-origin', body: part});
    if (!c.ok) throw new Error('chunk fallito ' + n);
    progress(file.name, n+1, Math.ceil(file.size/chunk_size));
  }
  await api(`/api/upload/${id}/finish`, {method:'POST'});
}
```
...e wiring: `blogin.click → api('/auth/login',{POST json}) → loadList()`; `breg` chiama `/auth/register` solo se `loadList` fallisce per 403 — nota UX: mostra bottone relative; `del(id)`; `list()` disegna <tr> con `filesize locale` + `cella Azioni` (Scarica − link a `/api/files/{id}/download`, Elimina).
- [ ] **Step 3: `web/style.css`** — flexbox semplice, card centrata, lista tabellare, responsive; palette sobria scura.›
- [ ] **Step 4: `server/src/handlers_static.rs`**:
```rust
pub async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(include_str!("../../web/index.html"))
}
pub async fn appjs() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "application/javascript")], include_str!("../../web/app.js"))
}
pub async fn style() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], include_str!("../../web/style.css"))
}
```
e in `build_router`:
```rust
.route("/", get(handlers_static::index))
.route("/app.js", get(handlers_static::appjs))
.route("/style.css", get(handlers_static::style))
```
CSS type text/html da `Html`, `text/css`, `application/javascript`.
- [ ] **Step 5: test smoke:**
```rust
#[tokio::test]
async fn index_served() {
    let t = boot().await;
    let res = t.app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), 200);
    let body = to_bytes(res.into_body()).await;
    assert!(!body.is_empty());
}
```
- [ ] **Step 6: run + fmt + clippy** — `cargo fmt && cargo clippy -D warnings && cargo test --workspace`
- [ ] **Step 7: Commit** — `feat: dashboard web embedded`

### Task 7: Deploy assets + tag

**Files:**
- Create: `deploy/vaulti.service`, `deploy/README.md`
**Interfaces:**
- Consumes: release GitHub Actions su tag → binario `vaulti-server` Linux.

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

- [ ] **Step 2: `deploy/README.md`** — passo-passo eseguito dall'utente col mio supporto:
```
1. Tecnologia gateway: Debian 12/13 o Ubuntu 22.04+ (se già Linux diverso: dimmelo)
2. su - guardato su utenti/dir:
   useradd -r -s /usr/sbin/nologin vaulti
   mkdir -p /mnt/data/vaulti && chown vaulti:vaulti /mnt/data/vaulti
3. Download:
   curl -L https://github.com/FenixsGame/vaulti/releases/download/vX.Y.Z/vaulti-server -o vaulti-server
   install -m755 vaulti-server /usr/local/bin/vaulti-server
4. Config:
   cp deploy/vaulti.service /etc/systemd/system/ ; systemctl daemon-reload ; systemctl enable --now vaulti
   journalctl -u vaulti -f
5. Tailscale (inst su gateway):
   curl -fsSL https://tailscale.com/install.sh | sh
   tailscale up
   tailscale serve --bg --https=443 http://127.0.0.1:8420
6. Primo accesso da browser tailnet: https://<nomemacchina>.tailnet.ts.net → "Primo utente (admin)"
```

- [ ] **Step 3: run + clippy + fmt + test** —
- [ ] **Step 4: tag** — `git tag v0.1.0 && git push origin v0.1.0` → Actions: eigene release build priporca; chiarire che la release già failed → GitHub Releases binario.
- [ ] **Step 5: Commit** — `chore: systemd + deploy docs`

---

## Self-review piani

**Dipendenze:** 1→2→3→4→5→6→7 sequenziale (Shared common/mod.rs e AppState vengono mutate per ogni task).
**Placeholders:** controlla e risolvi ogni sezione di test per intero (versione finale del codice, no stenografia).
