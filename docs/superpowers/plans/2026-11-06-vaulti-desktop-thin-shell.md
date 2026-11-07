# Vaulti Desktop (guscio sottile Tauri) — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** app Windows (Tauri v2) che mostra il dashboard del server in una webview same-origin, con tray, sessione persistente e installer NSIS da CI.

**Architecture:** guscio sottile — la finestra naviga direttamente l'URL del server (`https://<host>.ts.net`); nessuna modifica CORS/CSRF al server; config locale in `%APPDATA%`; capability Tauri senza campo `remote` (il contenuto remoto non ha IPC); icone + `Cargo.lock` committati; job Windows in CI per fmt/clippy/test e job `release-desktop` per l'installer su tag `desktop-v*`.

**Tech stack:** Tauri v2 (crate `tauri` + plugins `opener`, `single-instance`, `autostart`), `url`, `serde`, `reqwest` (rustls) solo lato Rust; crate esistente `vaulti-server` (axum + SQLite) con due piccole aggiunte.

**Spec:** `docs/superpowers/specs/2026-11-06-vaulti-desktop-thin-shell-design.md` (v.1.1, approvata)

## Global Constraints

- **Nessun cargo utile in locale** (manca link.exe su questa macchina): ogni verifica è **CI-only** — commit → push → `gh run watch` con `--exit-status`; al rosso si scarica il log con `gh run view <id> --log-failed`. Test e implementazione possono andare nello stesso push (compromesso RED-on-CI ratificato); un task è completo solo col run **verde**. `cargo generate-lockfile` è l'unico comando cargo locale permesso (risolve il grafo delle dipendenze senza compilare né linkare).
- Refresh PATH necessario per ogni shell: `[System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + ...('Path','User')` + `';' + "$env:USERPROFILE\.cargo\bin"` quando serve cargo.
- `cargo fmt --check` / `clippy -- -D warnings` girano SOLO in CI: seguire le lezioni del registro `.superpowers/sdd/2026-11-05-vaulti-mvp.md/progress.md` (let-chains verticali, indent di tutti i link della catena, uppercase-imports, SQL >100 col multiline).
- I tag già pubblicati non si muovono mai; desktop usa tag `desktop-v*`, il server usa `v0.1.1+` per i fix. Eccezione ristretta: un tag appena creato e non ancora scaricato da nessuno può essere spostato subito se il suo run è rosso; dopo il primo download vale la regola ferrea del tag patch nuovo.
- Versioni pinnate: `tauri-cli` v2.12.4, asset `cargo-tauri-x86_64-pc-windows-msvc.zip`, SHA-256 `3e8687fb184bd7f10ebd8642f7715f79f4a605c0694c142a21507e8a24dc0f8b`.
- Il crate `vaulti-server` resta con i suoi test verdi dopo ogni task; il guard CSRF e le regole cookie NON cambiano.
- Identificatore app stabile: `net.fenixs.vaulti`; productName `Vaulti`; NSIS `installMode: currentUser`.
- Si comunica all'utente in italiano, fine breve con una riga "non verificato/rischio".

## Review Focus

Input/failure modes che i test automatici non coprono; ognuno col test che lo blocca:

1. **UI stantia dopo upgrade server in WebView2** (asset senza hash + cache persistente della webview) → test `static_assets_no_cache` (Task 2).
2. **URL http o malformato salvato in config** (il campo arriva da una form locale: nessuna protezione naturale) → unit test di `validate_url` e del roundtrip `save_to`/`load_from` (Task 3).
3. **Server cade a app aperta** (health check solo pre-navigazione; reload diretto mostra l'errore nativo; l'uscita è la tray sempre presente) → checklist manuale spec §10.2, owner Task 5 (voci di menu Ricarica/Cambia server + X nascosto).
4. **IPC dal contenuto remoto** (il canale `__TAURI_INTERNALS__` esiste sempre; cambia chi lo può usare: il solo "fallisce" non prova la capability) → duplice verifica manuale spec §10.6: positivo (`save_and_connect` dalla pagina locale) e negativo (invoke rifiutato dalla pagina remota), owner Task 4.
5. **`target=_blank`/`window.open` non attraversa `on_navigation`** → unit test `allow` (Task 4) + verifica manuale da console pagina remota che l'apertura finisca nel browser di sistema (spec §10.7).

---

### Task 1: server `--version`

**Files:**
- Modify: `server/src/main.rs` (file intero, 7 righe oggi)
- Test: `server/tests/integration.rs` (in coda)

**Interfaces:**
- Consumes: `vaulti_server::reset_password()` e `vaulti_server::run()` (esistenti).
- Produces: `main` accetta argomento `--version` che stampa `vaulti-server <CARGO_PKG_VERSION>` ed esce 0. `reset-password` resta come è.

- [x] **Step 1: test in coda a `server/tests/integration.rs`**

```rust
#[test]
fn version_flag_prints_version() {
    use std::process::Command;
    let out = Command::new(env!("CARGO_BIN_EXE_vaulti-server"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("vaulti-server {}", env!("CARGO_PKG_VERSION"))
    );
}
```

Nota: `#[test]` sincrono, senza harness; `env!("CARGO_PKG_VERSION")` nel test coincide con quello del binario (stesso package `vaulti-server`).

- [x] **Step 2: sostituisci tutto `server/src/main.rs`**

```rust
fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("reset-password") => vaulti_server::reset_password(),
        Some("--version") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        }
        _ => vaulti_server::run(),
    }
}
```

(Le due funzioni terminano da sole: `reset_password` e `run` non ritornano.)

- [ ] **Step 3: commit, push, CI watch fino al verde**

```bash
git add server/src/main.rs server/tests/integration.rs
git commit -m "feat: flag --version per vaulti-server"
git push
```

Watch (pattern di sessione): `Start-Sleep -Seconds 20; $id = gh run list --repo FenixsGame/vaulti --limit 1 --json databaseId --jq '.[0].databaseId'; gh run watch $id --repo FenixsGame/vaulti --exit-status --interval 15`. Rosso → `gh run view <id> --log-failed` → fix → nuovo push (stesso task finché non verde).

- [ ] **Step 4: riga completa nel registro `progress.md` (commit, run id, esito).**

---

### Task 2: `Cache-Control: no-cache` su asset statici

**Files:**
- Modify: `server/src/handlers_static.rs` (file intero, 21 righe oggi)
- Test: `server/tests/integration.rs` (in coda)

**Interfaces:**
- Consumes: niente di nuovo (axum già in uso; tuple `[(HeaderName, &'static str); 2]` supportata).
- Produces: le route `/`, `/app.js`, `/style.css` rispondono con header `cache-control: no-cache` oltre al content-type attuale.

- [x] **Step 1: test in coda a `server/tests/integration.rs`**

```rust
#[tokio::test]
async fn static_assets_no_cache() {
    let t = common::boot().await;
    for path in ["/", "/app.js", "/style.css"] {
        let res = t
            .app
            .clone()
            .oneshot(common::raw_req("GET", path, "", Vec::new()))
            .await
            .unwrap();
        assert_eq!(
            res.headers().get("cache-control").and_then(|v| v.to_str().ok()),
            Some("no-cache"),
            "route {path}"
        );
    }
}
```

(cookie vuota va bene: le route statiche sono pubbliche.)

- [x] **Step 2: sostituisci tutto `server/src/handlers_static.rs`**

```rust
use axum::http::HeaderName;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};

pub async fn index() -> ([(HeaderName, &'static str); 2], &'static str) {
    (
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../web/index.html"),
    )
}

pub async fn appjs() -> ([(HeaderName, &'static str); 2], &'static str) {
    (
        [
            (CONTENT_TYPE, "application/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../web/app.js"),
    )
}

pub async fn style() -> ([(HeaderName, &'static str); 2], &'static str) {
    (
        [
            (CONTENT_TYPE, "text/css; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        include_str!("../../web/style.css"),
    )
}
```

(`Html` viene sostituito dalla tuple: aggiunge il secondo header senza layer.)

- [ ] **Step 3: commit, push, watch fino al verde**

```bash
git add server/src/handlers_static.rs server/tests/integration.rs
git commit -m "feat: cache-control no-cache su asset statici (desktop + browser)"
git push
```

- [ ] **Step 4: ledger.**

---

### Task 3: scheletro desktop compilabile + job CI Windows

**Files:**
- Modify: `Cargo.toml` (radice), `.github/workflows/ci.yml`
- Create: `desktop/ui/index.html`, `desktop/ui/error.html`, `desktop/icon.png`, `desktop/src-tauri/Cargo.toml`, `desktop/src-tauri/build.rs`, `desktop/src-tauri/tauri.conf.json`, `desktop/src-tauri/capabilities/default.json`, `desktop/src-tauri/icons/*` (generate), `desktop/src-tauri/src/main.rs`, `desktop/src-tauri/src/lib.rs`, `desktop/src-tauri/src/config.rs`, `desktop/src-tauri/.gitignore`, `desktop/src-tauri/Cargo.lock`
- Test: `desktop/src-tauri/src/config.rs` (mod `#[cfg(test)]`)

**Interfaces:**
- Consumes: niente.
- Produces (i Task 4-6 importano esattamente questi nomi):
  - crate `vaulti-desktop` (binario `vaulti`, tauri.conf `mainBinaryName` default dal productName);
  - `vaulti_desktop::config::{Config, validate_url, load_from, save_to}` con
    - `Config { server_url: String, autostart: bool }` (serde; `autostart` con `#[serde(default)]`),
    - `validate_url(s: &str) -> Result<String, String>` — solo https con host non vuoto, restituisce `Url::to_string()` (l'https senza path viene normalizzato con `/` finale),
    - `load_from(dir: &Path) -> Option<Config>` — file assente/corrotto/URL invalido → `None`,
    - `save_to(dir: &Path, c: &Config) -> std::io::Result<()>` — crea la dir se manca, scrive `config.json`;
  - finestra Tauri etichetta `main` (creata nel Task 4), identifier `net.fenixs.vaulti`, icone committate in `icons/`.

- [x] **Step 1: `Cargo.toml` radice (file completo)**

```toml
[workspace]
resolver = "2"
members = ["server"]
exclude = ["desktop"]
```

- [x] **Step 2: icone (locale, nessuna compilazione)**

```powershell
$env:Path = [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [System.Environment]::GetEnvironmentVariable('Path','User') + ';' + "$env:USERPROFILE\.cargo\bin"
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap(1024, 1024)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(255, 43, 45, 66))
$f = New-Object System.Drawing.Font('Segoe UI', 384, [System.Drawing.FontStyle]::Bold)
$sz = $g.MeasureString('V', $f)
$g.DrawString('V', $f, [System.Drawing.Brushes]::White, (1024 - $sz.Width) / 2, (1024 - $sz.Height) / 2)
New-Item -ItemType Directory -Force 'D:\AI PROJECT\Vaulti\desktop' | Out-Null
$bmp.Save('D:\AI PROJECT\Vaulti\desktop\icon.png', [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
curl.exe -sSL -o "$env:TEMP\cargo-tauri.zip" 'https://github.com/tauri-apps/tauri/releases/download/tauri-cli-v2.12.4/cargo-tauri-x86_64-pc-windows-msvc.zip'
if ((Get-FileHash "$env:TEMP\cargo-tauri.zip" -Algorithm SHA256).Hash.ToLower() -ne '3e8687fb184bd7f10ebd8642f7715f79f4a605c0694c142a21507e8a24dc0f8b') { throw 'hash mismatch' }
Expand-Archive "$env:TEMP\cargo-tauri.zip" -DestinationPath "$env:TEMP\tauricli" -Force
& "$env:TEMP\tauricli\cargo-tauri.exe" icon 'D:\AI PROJECT\Vaulti\desktop\icon.png' -o 'D:\AI PROJECT\Vaulti\desktop\src-tauri\icons'
```

(`desktop/icon.png` sorgente va committato: il README Task 6 spiega come rigenerare.)

- [x] **Step 3: `desktop/src-tauri/Cargo.toml`**

```toml
[package]
name = "vaulti-desktop"
version = "0.1.0"
edition = "2021"

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
url = "2"

[dev-dependencies]
tempfile = "3"
```

(reqwest e i plugin arrivano nei Task 4-5, ogni task resta compilabile da solo.)

- [x] **Step 4: `desktop/src-tauri/build.rs`**

```rust
fn main() {
    tauri_build::build()
}
```

- [x] **Step 5: `desktop/src-tauri/tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Vaulti",
  "identifier": "net.fenixs.vaulti",
  "version": "0.1.0",
  "build": { "frontendDist": "../ui" },
  "app": {
    "windows": [],
    "security": {
      "csp": "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'"
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": ["icons/icon.ico"],
    "windows": { "nsis": { "installMode": "currentUser" } }
  }
}
```

- [x] **Step 6: capability solo locale — `desktop/src-tauri/capabilities/default.json`**

```json
{
  "$schema": "https://schema.tauri.app/capability/2",
  "identifier": "default",
  "description": "invocazioni consentite solo al contenuto locale; nessun campo remote: il sito del server non ottiene IPC",
  "windows": ["main"],
  "local": true,
  "permissions": ["core:default"]
}
```

- [x] **Step 7: `desktop/src-tauri/.gitignore`**

```
/target
/gen/schemas
```

(MAI aggiungere `Cargo.lock`: lo commitiamo.)

- [x] **Step 8: `desktop/ui/index.html` e `desktop/ui/error.html` segnaposto** (il Task 4 li sostituisce; servono ora perché `frontendDist` punti a file reali)

```html
<!doctype html><meta charset="utf-8"><title>Vaulti</title><p>pagina Task 4</p>
```

(medesimo contenuto per error.html.)

- [x] **Step 9: `desktop/src-tauri/src/config.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    #[serde(default)]
    pub autostart: bool,
}

pub fn validate_url(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("URL vuoto".into());
    }
    let u = url::Url::parse(t).map_err(|e| format!("URL non valido: {e}"))?;
    if u.scheme() != "https" {
        return Err(format!("solo https (ricevuto '{}://')", u.scheme()));
    }
    if u.host_str().unwrap_or("").is_empty() {
        return Err("host mancante".into());
    }
    Ok(u.to_string())
}

pub fn load_from(dir: &Path) -> Option<Config> {
    let txt = std::fs::read_to_string(dir.join("config.json")).ok()?;
    let c: Config = serde_json::from_str(&txt).ok()?;
    validate_url(&c.server_url).ok()?;
    Some(c)
}

pub fn save_to(dir: &Path, c: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(c).expect("config serializzabile");
    std::fs::write(dir.join("config.json"), json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_valida_https() {
        assert_eq!(
            validate_url("https://nome.ts.net").unwrap(),
            "https://nome.ts.net/"
        );
    }

    #[test]
    fn url_http_rifiutato() {
        assert!(validate_url("http://nome.ts.net").is_err());
    }

    #[test]
    fn url_fuoriserie_rifiutati() {
        for s in ["", "   ", "https://", "ftp://x.ts.net", "nome.ts.net", "//x"] {
            assert!(validate_url(s).is_err(), "avrebbe accettato {s:?}");
        }
    }

    #[test]
    fn config_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        assert!(load_from(d.path()).is_none());
        let c = Config {
            server_url: "https://nome.ts.net/".into(),
            autostart: false,
        };
        save_to(d.path(), &c).unwrap();
        assert_eq!(load_from(d.path()).unwrap().server_url, "https://nome.ts.net/");
    }

    #[test]
    fn config_corrotta_o_url_invalido_none() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("config.json"), "{{{{{").unwrap();
        assert!(load_from(d.path()).is_none());
        let c = Config {
            server_url: "http://nome.ts.net/".into(),
            autostart: false,
        };
        save_to(d.path(), &c).unwrap();
        assert!(load_from(d.path()).is_none());
    }
}
```

- [x] **Step 10: `desktop/src-tauri/src/lib.rs` (minimale, il Task 4 sostituirà il file)**

```rust
pub mod config;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("errore avvio Vaulti");
}
```

- [x] **Step 11: `desktop/src-tauri/src/main.rs`**

```rust
fn main() {
    vaulti_desktop::run();
}
```

- [x] **Step 12: Cargo.lock locale**

```powershell
$env:Path = [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [System.Environment]::GetEnvironmentVariable('Path','User') + ';' + "$env:USERPROFILE\.cargo\bin"
Set-Location 'D:\AI PROJECT\Vaulti\desktop\src-tauri'
cargo generate-lockfile
```

- [x] **Step 13: job CI Windows — in `.github/workflows/ci.yml` aggiorna la sezione `on` e inserisci due job tra `test` e `release`**

La sezione `on` diventa: senza `desktop-v*` tra i tag il push del tag NON attiva il workflow (difetto trovato in questo self-review):

```yaml
on:
  push: { branches: [main], tags: ["v*", "desktop-v*"] }
  pull_request:
```

Poi aggiungi, dopo il job `test`:

```yaml
  changes:
    permissions: { contents: read }
    runs-on: ubuntu-latest
    outputs:
      desktop: ${{ steps.f.outputs.desktop }}
    steps:
      - uses: actions/checkout@v4
      - uses: dorny/paths-filter@v3
        id: f
        with:
          filters: |
            desktop:
              - 'desktop/**'
              - '.github/workflows/**'

  desktop:
    if: needs.changes.outputs.desktop == 'true' || startsWith(github.ref, 'refs/tags/desktop-v')
    needs: [changes]
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt, clippy }
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: desktop/src-tauri }
      - run: cargo fmt --check --all
        working-directory: desktop/src-tauri
      - run: cargo clippy -- -D warnings
        working-directory: desktop/src-tauri
      - run: cargo test
        working-directory: desktop/src-tauri
```

(sul push di un tag `desktop-v*` `paths-filter` ha base incerta: copre la seconda condizione della `if`; sui push normali funziona il diff contro la base standard, che copre `desktop/**` e `.github/workflows/**`.)

- [x] **Step 14: commit, push, watch fino a `test` + `desktop` verdi (prima build windows ~10-15 min)**

```bash
git add Cargo.toml .github/workflows/ci.yml desktop
git commit -m "feat: scheletro desktop Tauri (config, icone, capability locale) piu job CI windows"
git push
```

- [x] **Step 15: ledger.**

---

### Task 4: avvio, pagine locali, health check, guard navigazione

**Files:**
- Modify: `desktop/src-tauri/Cargo.toml` (aggiunge `reqwest`, `tauri-plugin-opener`), `desktop/src-tauri/src/lib.rs` (file completo), `desktop/ui/index.html`, `desktop/ui/error.html`
- Create: `desktop/src-tauri/src/nav.rs`
- Test: `desktop/src-tauri/src/nav.rs` (mod `#[cfg(test)]`)

**Interfaces:**
- Consumes: `config::{Config, load_from, save_to, validate_url}` (Task 3).
- Produces:
  - `nav::allow(cur: &url::Url, server_url: &str) -> bool` — true se l'URL è locale dell'app o coincide con l'origin del server configurato (`server_url` vuota → solo locale);
  - `nav::is_local(cur: &url::Url) -> bool` — true per `http://tauri.localhost` e `tauri://localhost`;
  - `nav::local_url(page: &str) -> String` — `http://tauri.localhost/<page>` (su Windows WebView2 l'origin locale è `http://tauri.localhost`);
  - `healthz_ok(base: &str) -> bool` (async) — `GET {base}/healthz`, timeout 3 s, reqwest rustls, solo Rust;
  - comandi (richiamabili SOLO dalle pagine locali; i tre sono `#[tauri::command]` **sync**, con `block_on` per la health):
    - `save_and_connect(url: String) -> Result<(), String>` — valida https, salva config, aggiorna lo stato server, healthz; ok → naviga al server; altrimenti `Err("URL non valida…")` o `Err("server irraggiungibile")`;
    - `retry_connect(app: AppHandle) -> Result<(), String>` — nessun save: legge config a disco, healthz, naviga o `Err("server irraggiungibile")`;
    - `open_config(app: AppHandle) -> Result<(), String>` — naviga `index.html`;
  - stato gestito `Mutex<String>` (l'URL server corrente, `""` se non configurato) usato dal guard `on_navigation`;
  - `on_navigation`: allow → passa; else → apre nel browser di sistema con `opener` (spawn async) e blocca.

- [x] **Step 1: `desktop/src-tauri/src/nav.rs` completo con unit test**

```rust
use url::Url;

/// true = navigazione permessa nella webview.
/// Locale dell'app sempre; origin del server se configurato; altro → false.
pub fn allow(cur: &Url, server_url: &str) -> bool {
    if is_local(cur) {
        return true;
    }
    if server_url.is_empty() {
        return false;
    }
    match Url::parse(server_url) {
        Ok(s) => cur.origin() == s.origin(),
        Err(_) => false,
    }
}

/// origin locale della webview su Windows (WebView2) e alias tauri.
pub fn is_local(cur: &Url) -> bool {
    const LOCAL: [(&str, &str); 2] = [("http", "tauri.localhost"), ("tauri", "localhost")];
    LOCAL
        .iter()
        .any(|(s, h)| cur.scheme() == *s && cur.host_str() == Some(*h))
}

/// URL di una pagina locale (frontendDist `../ui`).
pub fn local_url(page: &str) -> String {
    format!("http://tauri.localhost/{page}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn origin_server_passa() {
        let s = "https://nome.ts.net";
        for cur in [
            "https://nome.ts.net/",
            "https://nome.ts.net/app.js",
            "https://nome.ts.net/api/files?folder_id=",
            "https://nome.ts.net:443/api/me",
        ] {
            assert!(allow(&u(cur), s), "avrebbe bloccato {cur}");
        }
    }

    #[test]
    fn origin_simili_bloccati() {
        let s = "https://nome.ts.net";
        for cur in [
            "http://nome.ts.net/",
            "https://evil.example.com/",
            "https://nome.ts.net.attacker.com/",
            "https://attacker.com/?x=https://nome.ts.net",
        ] {
            assert!(!allow(&u(cur), s), "avrebbe permesso {cur}");
        }
    }

    #[test]
    fn pagine_locali_passano() {
        let s = "https://nome.ts.net";
        assert!(allow(&u("http://tauri.localhost/index.html"), s));
        assert!(allow(&u("tauri://localhost/error.html"), s));
    }

    #[test]
    fn senza_server_solo_locale() {
        assert!(!allow(&u("https://nome.ts.net/"), ""));
        assert!(allow(&u("http://tauri.localhost/index.html"), ""));
    }

    #[test]
    fn server_url_invalido_non_panica() {
        assert!(!allow(&u("https://nome.ts.net/"), ":::::"));
    }

    #[test]
    fn local_url_formato() {
        assert_eq!(
            local_url("error.html"),
            "http://tauri.localhost/error.html"
        );
    }
}
```

- [x] **Step 2: dipendenze in `desktop/src-tauri/Cargo.toml`**

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
tauri-plugin-opener = "2"
```

- [x] **Step 3: `Cargo.lock` rigenerato in locale**

```powershell
Set-Location 'D:\AI PROJECT\Vaulti\desktop\src-tauri'; cargo generate-lockfile
```

- [x] **Step 4: sostituisci tutto `desktop/src-tauri/src/lib.rs`**

```rust
pub mod config;
pub mod nav;

use std::sync::Mutex;
use std::time::Duration;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow};
use tauri_plugin_opener::OpenerExt;
use url::Url;

pub async fn healthz_ok(base: &str) -> bool {
    let Ok(cli) = reqwest::Client::builder().timeout(Duration::from_secs(3)).build()
    else {
        return false;
    };
    let url = format!("{}/healthz", base.trim_end_matches('/'));
    matches!(cli.get(url).send().await, Ok(r) if r.status().is_success())
}

fn navigate(w: &WebviewWindow, target: &str) -> Result<(), String> {
    let u = Url::parse(target).map_err(|e| e.to_string())?;
    w.navigate(u).map_err(|e| e.to_string())
}

fn window_of(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "finestra assente".into())
}

fn current_server(app: &AppHandle) -> String {
    app.state::<Mutex<String>>().lock().cloned().unwrap_or_default()
}

fn set_server(app: &AppHandle, s: &str) {
    if let Ok(mut g) = app.state::<Mutex<String>>().lock() {
        *g = s.to_string();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::<String>::new(String::new()))
        .invoke_handler(tauri::generate_handler![
            save_and_connect,
            retry_connect,
            open_config,
        ])
        .setup(|app| {
            let dir = app.path().app_config_dir().expect("config dir");
            let cfg = config::load_from(&dir);
            let start: WebviewUrl = match &cfg {
                Some(c) if tauri::async_runtime::block_on(healthz_ok(&c.server_url)) => {
                    set_server(app, &c.server_url);
                    WebviewUrl::External(Url::parse(&c.server_url).expect("url di config valida"))
                }
                Some(c) => {
                    set_server(app, &c.server_url);
                    WebviewUrl::App("error.html".into())
                }
                None => WebviewUrl::App("index.html".into()),
            };
            let apph = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", start)
                .title("Vaulti")
                .disable_dragdrop_handler()
                .on_navigation(move |url| {
                    if nav::allow(url, &current_server(&apph)) {
                        return true;
                    }
                    let ext = url.to_string();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = apph.opener().open_url(&ext, None::<&str>) {
                            eprintln!("open_url fallito: {e}");
                        }
                    });
                    false
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("errore avvio Vaulti");
}

#[tauri::command]
fn save_and_connect(app: AppHandle, url: String) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    // rispetta l'autostart già salvato: la riscrittura cambia solo l'URL
    let autostart = config::load_from(&dir)
        .map(|p| p.autostart)
        .unwrap_or(false);
    let c = config::Config {
        server_url: config::validate_url(&url)?,
        autostart,
    };
    config::save_to(&dir, &c).map_err(|e| e.to_string())?;
    set_server(&app, &c.server_url);
    if !tauri::async_runtime::block_on(healthz_ok(&c.server_url)) {
        return Err("server irraggiungibile".into());
    }
    let w = window_of(&app)?;
    navigate(&w, &c.server_url)
}

#[tauri::command]
fn retry_connect(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let c = config::load_from(&dir).ok_or("config assente")?;
    if !tauri::async_runtime::block_on(healthz_ok(&c.server_url)) {
        return Err("server irraggiungibile".into());
    }
    set_server(&app, &c.server_url);
    let w = window_of(&app)?;
    navigate(&w, &c.server_url)
}

#[tauri::command]
fn open_config(app: AppHandle) -> Result<(), String> {
    let w = window_of(&app)?;
    navigate(&w, &nav::local_url("index.html"))
}
```

- [x] **Step 5: `desktop/ui/index.html` (pagina configurazione, canale IPC reale `__TAURI_INTERNALS__`)**

```html
<!doctype html>
<html lang="it">
<head>
<meta charset="utf-8">
<title>Vaulti — collega il server</title>
<style>
  :root { color-scheme: dark light; }
  body { font-family: system-ui; display: grid; place-items: center; min-height: 100vh; margin: 0; background: #17171b; color: #eee; }
  main { display: grid; gap: 10px; min-width: 340px; }
  input, button { font-size: 16px; padding: 10px; border-radius: 8px; border: 1px solid #444; background: #222630; color: #eee; }
  button { background: #4c61ff; color: #fff; cursor: pointer; }
  #msg { color: #ff8989; min-height: 20px; }
  h1 { margin: 0 0 6px; }
</style>
</head>
<body>
<main>
  <h1>Vaulti</h1>
  <p>URL del server (solo https):</p>
  <input id="u" type="url" placeholder="https://nome.ts.net">
  <button id="go">Collega</button>
  <div id="msg"></div>
</main>
<script>
const msg = document.getElementById('msg');
// canale reale di Tauri v2 (controllo positivo del test IPC, spec §10.6)
const invoke = window.__TAURI_INTERNALS__.invoke;
document.getElementById('go').addEventListener('click', () => {
  msg.textContent = 'connessione in corso…';
  invoke('save_and_connect', { url: document.getElementById('u').value })
    .then(() => { msg.textContent = ''; })
    .catch((e) => { msg.textContent = String(e); });
});
</script>
</body>
</html>
```

- [x] **Step 6: `desktop/ui/error.html` (irraggiungibile: Riprova / Cambia server)**

```html
<!doctype html>
<html lang="it">
<head>
<meta charset="utf-8">
<title>Vaulti — server irraggiungibile</title>
<style>
  :root { color-scheme: dark light; }
  body { font-family: system-ui; display: grid; place-items: center; min-height: 100vh; margin: 0; background: #17171b; color: #eee; }
  main { display: grid; gap: 12px; min-width: 340px; text-align: center; }
  button { font-size: 16px; padding: 10px; border-radius: 8px; border: 1px solid #444; background: #2c2f36; color: #fff; cursor: pointer; }
  #msg { color: #ff8989; min-height: 20px; }
  h1 { margin: 0; }
</style>
</head>
<body>
<main>
  <h1>Server irraggiungibile</h1>
  <p>Controlla che Tailscale sia attivo e riprova.</p>
  <button id="retry">Riprova</button>
  <button id="cfg">Cambia server</button>
  <div id="msg"></div>
</main>
<script>
const invoke = window.__TAURI_INTERNALS__.invoke;
const msg = document.getElementById('msg');
document.getElementById('retry').addEventListener('click', () => {
  msg.textContent = 'riprovo…';
  invoke('retry_connect', {})
    .then(() => { msg.textContent = ''; })
    .catch((e) => { msg.textContent = String(e); });
});
document.getElementById('cfg').addEventListener('click', () => invoke('open_config', {}));
</script>
</body>
</html>
```

- [x] **Step 7: commit, push, watch fino a job desktop verde**

```bash
git add desktop
git commit -m "feat: flusso avvio desktop (config, healthz, pagina errore, guard navigazione)"
git push
```

(Gli errori più probabili: nome/metodo API v2 diverso da quello previsto — la correzione va eseguita col messaggio del compilatore e annotata nel registro.)

- [x] **Step 8: ledger (annota anche che il controllo positivo IPC è l'invoke di `save_and_connect`, la verifica negativa resta per il collaudo Task 7).**

---

### Task 5: tray, X nascosto, single-instance, autostart con retry

**Files:**
- Modify: `desktop/src-tauri/Cargo.toml` (aggiunge `tauri-plugin-single-instance`, `tauri-plugin-autostart`, `tokio` feature `time`), `desktop/src-tauri/src/lib.rs`
- Test: nessun unit nuovo; copertura = build CI + checklist manuale spec §10.2/.7/.8/.9 (owner Task 7)

**Interfaces:**
- Consumes: comandi Task 4, finestra `main`, `config::{Config, load_from}` con campo `autostart`.
- Produces:
  - tray id `vaulti-tray`, menu con identificatori: `open` ("Apri Vaulti"), `reload` ("Ricarica"), `cfg` ("Cambia server"), `quit` ("Esci"); click sinistro sull'icona = `open`;
  - evento `CloseRequested` → `api.prevent_close()` + `hide()` (X nasconde, non chiude);
  - secondo processo → mostra/attiva la finestra esistente;
  - `config.autostart == true` → `autostart().enable()`, finestra parte nascosta, retry loop con backoff 10 s → raddoppio fino a tetto 300 s; false → `disable()`.

- [x] **Step 1: dipendenze in `Cargo.toml`**

```toml
tauri-plugin-single-instance = "2"
tauri-plugin-autostart = "2"
tokio = { version = "1", features = ["time"] }
```

- [x] **Step 2: `Cargo.lock` rigenerato**

```powershell
Set-Location 'D:\AI PROJECT\Vaulti\desktop\src-tauri'; cargo generate-lockfile
```

- [x] **Step 3: modifiche chirurgiche a `desktop/src-tauri/src/lib.rs` (file del Task 4): i blocchi seguenti vanno nelle posizioni indicate**

1. Nella catena del builder, dopo `.plugin(tauri_plugin_opener::init())`, aggiungi i due plugin:

```rust
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
```

2. Nel builder della finestra aggiungi `on_window_event` subito dopo `disable_dragdrop_handler()`:

```rust
                .on_window_event(|w, ev| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = ev {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                })
```

3. Prima di `WebviewWindowBuilder`, nel `setup` del file di Task 4 (subito dopo i `match` che impostano `start`):

```rust
            let autostart = cfg.as_ref().map(|c| c.autostart).unwrap_or(false);
```

4. Aggiorna la testa del builder: con autostart la finestra nasce già nascosta, senza flash all'avvio (il resto della catena resta):

```rust
            WebviewWindowBuilder::new(app, "main", start)
                .title("Vaulti")
                .visible(!autostart)
```

5. In fondo al setup, dopo `.build()?` (il blocco sostituisce l'`Ok(())` che chiude il setup di Task 4):

```rust
            {
                use tauri_plugin_autostart::ManagerExt;
                let _ = if autostart {
                    app.autostart().enable()
                } else {
                    app.autostart().disable()
                };
            }
            if autostart {
                spawn_retry_loop(app.handle().clone());
            }
            use tauri::menu::{Menu, MenuItem};
            use tauri::tray::TrayIconBuilder;
            let open = MenuItem::with_id(app, "open", "Apri Vaulti", true, None::<&str>)?;
            let reload = MenuItem::with_id(app, "reload", "Ricarica", true, None::<&str>)?;
            let cfgmi = MenuItem::with_id(app, "cfg", "Cambia server", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Esci", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &reload, &cfgmi, &quit])?;
            TrayIconBuilder::with_id("vaulti-tray")
                .menu(&menu)
                .icon(app.default_window_icon().expect("icona bundle").clone())
                .on_menu_event(|app, ev| match ev.id().as_ref() {
                    "open" => activate(app),
                    "reload" => {
                        let _ = retry_connect(app.clone());
                    }
                    "cfg" => {
                        let _ = open_config(app.clone());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = ev
                    {
                        activate(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
```

6. Due funzioni nuove in fondo a `lib.rs`:

```rust
fn activate(app: &AppHandle) {
    if let Ok(w) = window_of(app) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn spawn_retry_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let dir = app.path().app_config_dir().expect("config dir");
        let Some(c) = config::load_from(&dir) else { return };
        let mut wait = 10;
        loop {
            if healthz_ok(&c.server_url).await {
                break;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait = (wait * 2).min(300);
        }
    });
}
```

(Con autostart, il retry in background assorbe il boot lento di Tailscale: la finestra parte nascosta e resta in uno stato gestito; quando il server risponde, l'utente apre dalla tray e il pulsante Riprova ora va a buon fine.)

- [x] **Step 4: commit, push, watch fino a desktop job verde**

```bash
git add desktop
git commit -m "feat: tray Apri/Ricarica/Cambia server/Esci, X nascosto, single-instance, autostart con retry"
git push
```

- [x] **Step 5: ledger.**

---

### Task 6: job `release-desktop` + README desktop

**Files:**
- Modify: `.github/workflows/ci.yml` (aggiunge `release-desktop` dopo il job `desktop`), `README.md` (append sezione)
- Test: verifica della release (Task 7, tag `desktop-v0.1.0`)

**Interfaces:**
- Consumes: crate completo (Task 3-5), icone in `icons/` (Task 3), `tauri.conf.json` con `version: "0.1.0"` e `installMode: currentUser` (Task 3), tag `desktop-v*`.
- Produces: release GitHub `desktop-vX.Y.Z` con asset `vaulti-setup-x64.exe`; sezione README "App Windows (desktop)" con nota SmartScreen e percorso dei download.

- [x] **Step 1: job in `ci.yml` (dopo il job `desktop`)**

```yaml
  release-desktop:
    if: startsWith(github.ref, 'refs/tags/desktop-v')
    needs: [desktop]
    runs-on: windows-latest
    permissions: { contents: write }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: desktop/src-tauri }
      - name: install tauri-cli (precompilata, pin + sha256)
        shell: pwsh
        run: |
          $ver = 'tauri-cli-v2.12.4'
          $hash = '3e8687fb184bd7f10ebd8642f7715f79f4a605c0694c142a21507e8a24dc0f8b'
          curl.exe -sSL -o cli.zip "https://github.com/tauri-apps/tauri/releases/download/$ver/cargo-tauri-x86_64-pc-windows-msvc.zip"
          if ((Get-FileHash cli.zip -Algorithm SHA256).Hash.ToLower() -ne $hash) { throw 'tauri-cli hash mismatch' }
          Expand-Archive cli.zip -DestinationPath cli-bin -Force
          Add-Content $env:GITHUB_PATH "$((Resolve-Path .).Path)\cli-bin"
      - name: riscrivi versione dal tag
        shell: pwsh
        run: |
          $v = $env:GITHUB_REF_NAME -replace '^desktop-v',''
          $p = 'desktop/src-tauri/tauri.conf.json'
          (Get-Content $p -Raw).Replace('"version": "0.1.0"', ('"version": ' + '"' + $v + '"')) | Set-Content $p -NoNewline
      - run: cargo tauri build --bundles nsis
        working-directory: desktop/src-tauri
      - name: rename asset
        shell: pwsh
        run: |
          $exe = Get-ChildItem 'desktop/src-tauri/target/release/bundle/nsis' -Filter '*-setup.exe' | Select-Object -First 1
          Copy-Item $exe.FullName vaulti-setup-x64.exe
      - uses: softprops/action-gh-release@v2
        with: { files: vaulti-setup-x64.exe }
```

(bump futuro della CLI: cambiare in questo job solo le due variabili `$ver` e `$hash`. La riscrittura della versione tocca la copia di `tauri.conf.json` nel checkout dei runner di release; nel repo il file resta a `0.1.0`.)

- [x] **Step 2: README — in fondo a `README.md`, aggiungi la sezione**

```markdown
## App Windows (desktop)

- Scarica `vaulti-setup-x64.exe` da una release `desktop-v*` del repo.
- **SmartScreen**: l'installer non è firmato, Windows può avvisare;
  **Maggiori informazioni → Esegui comunque**.
- Al primo avvio: inserire l'URL del server (solo `https://`, es.
  `https://nome.ts.net`).
- **Login**: avviene nella pagina del server; il client non salva credenziali
  (sessione = cookie, vale 30 giorni).
- **Download**: i file finiscono **nella cartella `Download`** dell'utente
  (barra di download della webview; nessun dialogo "salva con nome").

- **Icone**: per rigenerarle dopo un nuovo logo, parti da `desktop/icon.png`
  con `cargo tauri icon desktop/icon.png -o desktop/src-tauri/icons` (CLI
  `tauri-cli-v2.12.4` precompilata, vedi workflow `release-desktop`).
```

- [x] **Step 3: commit, push, watch (release-desktop non gira: nessun tag desktop, il green del job `desktop` basta)**

```bash
git add .github/workflows/ci.yml README.md
git commit -m "ci: release-desktop per tag desktop-v* (CLI pinnata+hash, rename asset); README desktop"
git push
```

- [x] **Step 4: ledger.**

---

### Task 7: tag `desktop-v0.1.0` + rilascio + handoff manuale

**Files:**
- Nessuna modifica codice (tag + verifica)
- Test: job `desktop` (per il tag) + `release-desktop`; poi checklist manuale spec §10 (utente reale)

**Interfaces:**
- Consumes: repo al Task 6 completato, tutto verde.
- Produces: release `desktop-v0.1.0` con `vaulti-setup-x64.exe`; collaudo completo dell'utente.

- [ ] **Step 1: tag + push + watch**

```powershell
git tag desktop-v0.1.0
git push origin desktop-v0.1.0
Start-Sleep -Seconds 20
$id = gh run list --repo FenixsGame/vaulti --limit 1 --json databaseId --jq '.[0].databaseId'
gh run watch $id --repo FenixsGame/vaulti --exit-status --interval 30
```

Atteso: job `desktop` (fmt/clippy/test) e `release-desktop` verdi.
Al rosso: se il tag NON è ancora stato scaricato da nessuno (release appena nata), fix e sposta subito il tag (lezione v0.1.0); in qualunque altro caso → tag patch nuovo (`desktop-v0.1.1`), con la regola annotata nel registro.

- [ ] **Step 2: verifica release + l'utente scarica e installa**

```powershell
gh release view desktop-v0.1.0 --repo FenixsGame/vaulti --json assets,tagName
```

Atteso: asset `vaulti-setup-x64.exe`. Installazione: doppio click; a SmartScreen "Maggiori informazioni → Esegui comunque" (currentUser, nessun UAC).

- [ ] **Step 3: checklist manuale all'utente (spec §10, tutti i 9 punti, incluso il duplice test IPC e `window.open` da console); risultati annotati nel registro.**

- [ ] **Step 4: ledger — riga COMPLETE con run id della release ed esiti del collaudo manuale; eventuali follow-up in lista nel registro.**
