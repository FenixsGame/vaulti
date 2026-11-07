# Vaulti UI redesign — Fase 1 (Fondamenta) — piano di esecuzione

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (esecuzione integrata ratificata dall'utente; flusso main diretto + CI-only, RED-on-CI). Step con `- [ ]`.

**Goal:** la dashboard cambia faccia (stile Windows 11/OneDrive) conservando TUTTE le funzioni attuali, e il server serve una web multi-file con no-cache.

**Architecture:** primo restyle in-place (stesso app.js, stessi id, rotte immutate) per un commit deployabile; poi swap atomico a struttura modulare (`css/`, `js/`, sprite inline) + handler `rust-embed` con MIME/no-cache/nosniff; infine session-utente in topbar + barra spazio. Nessun framework, nessun build, nessun CDN.

**Tech stack:** CSS custom properties, ES modules nativi, rust-embed 8 su axum, `node --test` in CI.

**Spec:** `docs/superpowers/specs/2026-11-07-vaulti-ui-redesign-design.md` (§2 layout, §3 token, §4 struttura, Fase 1).

## Global Constraints

- Nessun framework, NESSUNA build, nessun CDN; font di sistema (`"Segoe UI Variable", "Segoe UI", system-ui, sans-serif`, base 14px).
- Stessa UI per browser e app desktop; niente IPC dal contenuto remoto (niente barra titolo custom).
- Token CSS ESATTI (spec §3): `--bg-app #F3F3F3`, `--surface #FFFFFF`, `--border #E5E5E5`, `--text #1A1A1A`, `--text-2 #616161`, `--accent #0067C0` (hover `#1975C5`), `--row-hover rgba(0,0,0,.04)`, `--row-selected #E5F1FB`, `--danger #C42B1C`; raggi 4px controlli / 8px finestre-menu; riga 40px; anello focus 2px accent; transizioni 120ms con `prefers-reduced-motion` disattivate.
- Verifica CI-only (niente cargo run/build locale; ammessi SOLO `cargo fmt` + `cargo generate-lockfile`); flusso commit → push → watch; ogni task = run verde nel ledger.
- Delete = hard delete: nessuna azione distruttiva a un solo clic senza conferma (Fase 1: le conferme attuali `prompt()`/elimina diretta restano; le finestre di conferma vere arrivano in Fase 3).
- Il piano NON ridefinisce le API: tutte le chiamate esistenti (files/folders/upload) restano identiche.

## Review Focus

- Percorso non canonico nel fallback (`//x`, `..%2F`, backslash): mai fuori da `web/`, mai consentiti `..` — TEST su `/../x%2Fx` → 404 (Task 3).
- Metodo non-GET sul fallback (POST/PUT a path sconosciuto) → 404, non la pagina html — TEST (Task 3).
- Cache WebView2 vecchia dopo lo swap: ogni assetanswer con `Cache-Control: no-cache` — già testato per 2 endpoint? esteso a 3 (Task 3).
- MIME sbagliato (`text/plain` su `.js`): WebView2 rifiuta i moduli → `text/javascript` testato (Task 3).
- Server giù/Tailscale spento al primo via: niente form vuoto muto — messaggio sul card login (Task 4, manuale).

---

### Task 1: `format.js` (logica pura) + job CI `node --test`

**Files:**
- Create: `web/package.json`
- Create: `web/js/format.js`
- Create: `web/js/format.test.mjs`
- Modify: `.github/workflows/ci.yml` (aggiungere job `ui`)

**Interfaces:**
- Produces: `export function fmtBytes(n: number): string`, `export function fmtDate(iso: string): string` — Task 3 (`web/js/app.js`) le importa.

- [ ] **Step 1: file**

```json
// web/package.json — marca i .js come ES module (serve anche a node --test)
{ "type": "module" }
```

```js
// web/js/format.js — logica pura, senza DOM: testabile con node --test
export function fmtBytes(n) {
  return n >= 1073741824 ? (n / 1073741824).toFixed(1) + " GB"
    : n >= 1048576 ? (n / 1048576).toFixed(1) + " MB"
    : n >= 1024 ? (n / 1024).toFixed(1) + " KB"
    : n + " B";
}

export function fmtDate(iso) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString();
}
```

```js
// web/js/format.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { fmtBytes, fmtDate } from "./format.js";

test("fmtBytes", () => {
  assert.equal(fmtBytes(0), "0 B");
  assert.equal(fmtBytes(999), "999 B");
  assert.equal(fmtBytes(1024), "1.0 KB");
  assert.equal(fmtBytes(1073741824), "1.0 GB");
});

test("fmtDate", () => {
  assert.ok(fmtDate("2026-11-07T10:00:00").includes("2026"));
  assert.equal(fmtDate("non data"), "");
});
```

- [ ] **Step 2: job CI (in coda, sibling di `test`)**

```yaml
  ui:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: node --test web/js/
```

- [ ] **Step 3: commit, push, watch**

```bash
git add web .github/workflows/ci.yml
git commit -m "feat: format.js pura + job node --test (logica dati testabile)"
git push
```

- [ ] **Step 4: ledger (run id + esito).**

---

### Task 2: restyle in-place (token, reset, componenti, guscio pagina, icone)

Stesso `app.js`, stessi id, rotte immutate: commit deployabile con il look nuovo. Le classi nuove non toccano la logica.

**Files:**
- Rewrite: `web/index.html`
- Rewrite: `web/style.css`

**Interfaces:**
- Consumes: niente (solo markup/css).
- Produces: struttura DOM con id storici (`login, main, crumbs, listing, mvsel, quota, bpick, bfolder, file, prog, merr, blogout, blogin, breg, lerr, email, pw`) + id nuovi (`who, lnav-files, spacebar`).

- [ ] **Step 1: `web/index.html`**

```html
<!doctype html>
<html lang="it"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Vaulti</title>
<link rel="stylesheet" href="/style.css"></head><body>
<!-- sprite SVG: disegnate da zero, niente icone Microsoft copiate -->
<svg hidden xmlns="http://www.w3.org/2000/svg">
  <symbol id="i-logo" viewBox="0 0 16 16"><rect x="1" y="1" width="14" height="14" rx="3.5" fill="#0067C0"/><rect x="3.2" y="6.6" width="5.6" height="5.6" rx="1.2" fill="#fff"/><circle cx="10.6" cy="5.4" r="2.6" fill="#fff"/></symbol>
  <symbol id="i-folder" viewBox="0 0 16 16"><path d="M1.5 4.5a1.5 1.5 0 0 1 1.5-1.5h3l1.4 1.5h5.6a1.5 1.5 0 0 1 1.5 1.5v6a1.5 1.5 0 0 1-1.5 1.5h-10a1.5 1.5 0 0 1-1.5-1.5z" fill="currentColor" opacity=".85"/></symbol>
  <symbol id="i-file" viewBox="0 0 16 16"><path d="M4 1.5h5l3 3v10a.5.5 0 0 1-1 0V5H7.5V3H5v9.5h-.5A.75.75 0 0 1 4 11.75z" fill="currentColor" opacity=".85"/></symbol>
</svg>

<section id="login" class="login-overlay">
  <form class="login card" onsubmit="return false">
    <svg class="logo i32"><use href="#i-logo"/></svg>
    <h1>Vaulti</h1>
    <p class="sub">I tuoi file, in casa tua.</p>
    <input id="email" type="email" placeholder="email" autocomplete="username">
    <input id="pw" type="password" placeholder="password" autocomplete="current-password">
    <button id="blogin" class="btn primary wide">Accedi</button>
    <button id="breg" class="btn wide" hidden>Primo utente (admin)</button>
    <p id="lerr" class="err"></p>
  </form>
</section>

<main id="main" hidden>
  <header class="topbar">
    <div class="brand"><svg class="i i20"><use href="#i-logo"/></svg>Vaulti</div>
    <input id="q" class="search" type="search" placeholder="Cerca" disabled>
    <div class="user">
      <span id="who" class="who"></span>
      <button id="blogout" class="btn ghost">Esci</button>
    </div>
  </header>
  <div class="cols">
    <nav class="left">
      <button id="lnav-files" class="lnav-item active"><svg class="i i16"><use href="#i-folder"/></svg>I miei file</button>
      <div class="space">
        <div class="bar"><div id="spacebar" class="fill"></div></div>
        <span id="quota" class="quota"></span>
      </div>
    </nav>
    <div class="main">
      <nav id="crumbs" class="crumbs"></nav>
      <div class="actions">
        <button id="bpick" class="btn primary">Carica file</button>
        <button id="bfolder" class="btn">Nuova cartella</button>
        <label>Sposta in <select id="mvsel"></select></label>
        <input id="file" type="file" hidden multiple>
      </div>
      <table id="listing"><tbody></tbody></table>
      <p id="prog" class="prog"></p>
      <p id="merr" class="err"></p>
    </div>
  </div>
</main>
<script src="/app.js"></script></body></html>
```

- [ ] **Step 2: `web/style.css` (4 sezioni: token/reset, componenti, guscio, listing)**

```css
/* ==== token (spec §3) ==== */
:root {
  --bg-app: #F3F3F3; --surface: #FFFFFF; --border: #E5E5E5;
  --text: #1A1A1A; --text-2: #616161;
  --accent: #0067C0; --accent-hover: #1975C5;
  --row-hover: rgba(0,0,0,.04); --row-selected: #E5F1FB; --danger: #C42B1C;
  --r-ctl: 4px; --r-win: 8px; --row-h: 40px;
  --font: "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
}
/* ==== reset + base ==== */
* { box-sizing: border-box; margin: 0; }
html, body { height: 100%; }
body { font: 14px/1.4 var(--font); color: var(--text); background: var(--bg-app); }
button, input, select { font: inherit; color: inherit; }
:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
.i, svg { fill: currentColor; }
.i16 { width: 16px; height: 16px; } .i20 { width: 20px; height: 20px; } .i32 { width: 32px; height: 32px; }
@media (prefers-reduced-motion: reduce) { * { transition: none !important; animation: none !important; } }
/* ==== componenti ==== */
.btn { display: inline-flex; align-items: center; gap: 6px; padding: 5px 12px;
  background: var(--surface); border: 1px solid var(--border); border-radius: var(--r-ctl);
  transition: background-color 120ms; cursor: pointer; }
.btn:hover { background: #F5F5F5; }
.btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
.btn.primary:hover { background: var(--accent-hover); }
.btn.ghost { border-color: transparent; background: transparent; }
.btn.ghost:hover { background: var(--row-hover); }
.btn.wide { justify-content: center; width: 100%; }
.card { background: var(--surface); border: 1px solid var(--border); border-radius: var(--r-win);
  padding: 24px; box-shadow: 0 8px 24px rgba(0,0,0,.12); }
input[type=email], input[type=password], input[type=search], select {
  padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--r-ctl); background: var(--surface); }
input:disabled { background: var(--bg-app); color: var(--text-2); }
.err { color: var(--danger); min-height: 1.4em; font-size: 12px; }
/* ==== guscio ==== */
.login-overlay { min-height: 100vh; display: grid; place-items: center; padding: 16px; }
.login { width: min(360px, 100%); display: grid; gap: 10px; text-align: center; justify-items: stretch; }
.login .logo { margin: 0 auto; }
.login .sub { color: var(--text-2); margin-bottom: 8px; }
.topbar { display: flex; align-items: center; gap: 12px; height: 48px; padding: 0 16px;
  background: var(--surface); border-bottom: 1px solid var(--border); }
.brand { display: flex; align-items: center; gap: 8px; font-weight: 600; }
.search { flex: 1; max-width: 460px; margin: 0 auto; }
.user { display: flex; align-items: center; gap: 8px; margin-left: auto; }
.who { color: var(--text-2); }
.cols { display: flex; height: calc(100vh - 48px); }
.left { width: 220px; padding: 12px 8px; display: flex; flex-direction: column; gap: 2px; }
.lnav-item { display: flex; align-items: center; gap: 10px; padding: 7px 10px; border: 0;
  border-radius: var(--r-ctl); background: transparent; text-align: left; cursor: pointer; }
.lnav-item:hover { background: var(--row-hover); }
.lnav-item.active { background: var(--row-selected); }
.space { margin-top: auto; padding: 10px; }
.bar { height: 6px; border-radius: 3px; background: var(--row-hover); overflow: hidden; }
.fill { height: 100%; width: 0; background: var(--accent); transition: width 120ms; }
.quota { display: block; margin-top: 6px; color: var(--text-2); font-size: 12px; }
.main { flex: 1; min-width: 0; padding: 12px 20px; overflow: auto; }
.crumbs { display: flex; align-items: center; gap: 4px; color: var(--text-2); }
.crumbs button { border: 0; background: transparent; color: var(--accent); cursor: pointer;
  padding: 4px 6px; border-radius: var(--r-ctl); }
.crumbs button:hover { background: var(--row-hover); }
.actions { display: flex; align-items: center; gap: 10px; margin: 12px 0; }
.actions label { color: var(--text-2); display: flex; gap: 6px; align-items: center; }
.prog { color: var(--text-2); font-size: 12px; }
/* ==== listing ==== */
#listing { width: 100%; border-collapse: collapse; }
#listing td { height: var(--row-h); border-bottom: 1px solid var(--border); padding: 0 8px; }
#listing tr:hover td { background: var(--row-hover); }
#listing td button { margin-left: 4px; }
#listing a { color: var(--text); text-decoration: none; }
#listing a:hover { color: var(--accent); text-decoration: underline; }
```

- [ ] **Step 3: commit, push, watch**

```bash
git add web/index.html web/style.css
git commit -m "feat: guscio UI stile Win11/OneDrive, token CSS, sprite inline (id storici preservati)"
git push
```

- [ ] **Step 4: ledger.** La vecchia UI dal dispositivo non rompe: `no-cache` (T2 server) porta via subito le nuove pagine.

---

### Task 3: swap atomico — handler multi-file `rust-embed` + moduli ES

**Files:**
- Modify: `server/Cargo.toml` (+ `rust-embed = "8"`)
- Rewrite: `server/src/handlers_static.rs`
- Modify: `server/src/lib.rs` (rotte: tolte `/app.js` `/style.css`; aggiunto `.fallback(s::asset)`)
- Modify: `server/tests/integration.rs` (2 blocchi su rotte statiche aggiornati + test MIME/404/metodo)
- Modify: `web/index.html` (solo 2 righe in testa: css/js percorsi nuovi)
- Create: `web/css/tokens.css`, `web/css/base.css`, `web/css/components.css`, `web/css/layout.css` (split 1:1 del file unico del Task 2)
- Create: `web/js/api.js`, `web/js/app.js` (app.js = logica attuale invariata, import da api/format)
- Delete: `web/style.css`, `web/app.js`

**Interfaces:**
- Consumes: `fmtBytes` (Task 1), `/api/files` ecc. (immutate).
- Produces: route `/` (html), GET fallback su qualunque file embedded di `web/`; MIME `text/javascript` per `.js` (npm modules ok in WebView2).

- [ ] **Step 1: ruoli statici aggiornati — codice**

```toml
# server/Cargo.toml — in [dependencies]
rust-embed = "8"
```

```rust
// server/src/handlers_static.rs — embed della cartella web/, MIME per estensione, no-cache
use axum::extract::Request;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../web"]
struct Ui;

const NO_CACHE: &str = "no-cache";
const NOSNIFF: &str = "x-content-type-options";

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "html" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

pub async fn index() -> Response {
    let Some(f) = Ui::get("index.html") else { return miss().into_response() };
    asset_response("index.html", &f.data)
}

pub async fn asset(req: Request) -> Response {
    if req.method() != axum::http::Method::GET {
        return miss();
    }
    let path = req.uri().path().trim_start_matches('/');
    // solo file dentro web/: niente "..", niente backslash, niente campo vuoto
    if path.is_empty() || path.contains("..") || path.contains('\\') {
        return miss();
    }
    match Ui::get(path) {
        Some(f) => asset_response(path, &f.data),
        None => miss(),
    }
}

fn asset_response(_name: &str, data: &[u8]) -> Response {
    (
        [(CONTENT_TYPE, mime(_name)), (CACHE_CONTROL, NO_CACHE), ("x-content-type-options", "nosniff")],
        data,
    )
        .into_response()
}

fn miss() -> Response {
    (StatusCode::NOT_FOUND, [(CONTENT_TYPE, "text/plain"), (CACHE_CONTROL, NO_CACHE)], "").into_response()
}
```

(Rimangono i test 404: `GET /nope` → 404; `POST /nope` → 404 pleine; `GET /../x%2Fx` → 404; `GET /js/app.js` MIME `text/javascript`; nessun `Content-Length` duty-basata.)

Routes in `lib.rs`:

```rust
        .route("/", get(s::index))
        // .route("/app.js", ...) e .route("/style.css", ...) RIMOSSI
        .fallback(s::asset)
```

- [ ] **Step 2: moduli (split css 1:1; nuova firma head di index.html)**

```html
<link rel="stylesheet" href="/css/tokens.css">
<link rel="stylesheet" href="/css/base.css">
<link rel="stylesheet" href="/css/components.css">
<link rel="stylesheet" href="/css/layout.css">
...
<script type="module" src="/js/app.js"></script>
```

```js
// web/js/api.js
import { fmtBytes } from "./format.js";
export const $ = (id) => document.getElementById(id);
export async function api(p, opts = {}) {
  const r = await fetch(p, { credentials: "same-origin", ...opts });
  if (r.status === 401) { showLogin(); throw new Error("401"); }
  if (!r.ok) {
    const e = await r.json().catch(() => ({ error: `HTTP ${r.status}` }));
    throw new Error(e.error || `HTTP ${r.status}`);
  }
  return r.status === 204 ? null : r.json();
}
export function showLogin() { $("main").hidden = true; $("login").hidden = false; $("breg").hidden = false; }
export const fmt = fmtBytes;
```

`web/js/app.js` = il vecchio `web/app.js` (righe 1–259) con le 3 definizioni `$, api, showLogin, fmt` rimosse e in cima:

```js
import { $, api, showLogin, fmt } from "./api.js";
```

Il resto del codice (listit/render/upload/wiring/session-restore) NON cambia.

- [ ] **Step 3: test di integrazione (sostituzioni nei 2 blocchi + aggiunte)**

```rust
// GET /nope = 404 · POST /css/base.css = 404 (non l'html) · GET "/../x%2Fx" = 404
// GET /js/app.js: Content-Type "text/javascript"
```

- [ ] **Step 4: fmt locale (`cargo fmt`. lockfile SOLO se rust-embed serve: `cargo generate-lockfile`), commit, push, watch**

```bash
git add -A web server
git commit -m "feat: web multi-file servita da rust-embed (MIME, no-cache, nosniff, fallback sicuro); moduli ES"
git push
```

- [ ] **Step 5: ledger.** RED probabile: derive/feature di rust-embed o firma del fallback — fix dal log CI.

---

### Task 4: sessione in topbar, spazio, prima impresione login

**Files:**
- Modify: `web/js/app.js` (piccoli incremental su wiring)

**Interfaces:**
- Consumes: `/api/me` → `{email, is_admin}` (server, esistente); id `who`, `spacebar`.

- [ ] **Step 1: wiring (modulo app.js)** — `paintSpace(j)` messo SOLO dove `listit` già possiede la risposta:

```js
// dentro listit(j): la riga vecchia quota/.textContent va sostituita con
function paintSpace(j) {
  const tot = j.quota.used + j.quota.free;
  $("spacebar").style.width = tot ? Math.round((j.quota.used / tot) * 100) + "%" : "0";
  $("quota").textContent = `usati ${fmt(j.quota.used)} su ${fmt(tot)}`;
}
// in listit, dopo render(j): paintSpace(j);
```

- [ ] **Step 2: chi è connesso + messaggio pronta senza server (il blocco finale di app.js è riscritto)**

```js
// chi è connesso: 401 → la login card resta (api() chiude già showLogin)
api("/api/me").then((j) => { $("who").textContent = j.email; }).catch(() => {});

// al via: sessione valida → si entra diretto; server giù → messaggio in login card
listit(null)
  .then(() => { $("login").hidden = true; $("main").hidden = false; })
  .catch((e) => {
    if (String(e.message) !== "401")
      $("lerr").textContent = "server non raggiungibile: controlla Tailscale";
  });
```

- [ ] **Step 2: messaggio login quando il server non risponde (prima via)**

```js
api("/api/files")
  .then(() => { $("login").hidden = true; $("main").hidden = false; })
  .catch((e) => { if (String(e.message) !== "401") $("lerr").textContent = "server non raggiungibile: controlla Tailscale"; });
```

- [ ] **Step 3: commit, push, watch**

```bash
git add web/js/app.js
git commit -m "feat: sessione in topbar via /api/me, barra spazio, messaggio server irraggiungibile"
git push
```

- [ ] **Step 4: ledger + checklist manuale Fase 1 completa (browser E app desktop).**

## Checklist manuale Fase 1 (spec: "fatto quando")

1. Login via browser: card centrata, brand, hover del bottone accent; primo-utente visibile se vuoto DB (nuovo server di test).
2. Tutte le funzioni di prima dentro l'uso nuovo: lista, apri cartella, breadcrumb, crea/rinomina/elimina cartella, carica file con progresso, rinomina/sposta/elimina file, scarica, esci.
3. Sessione valida: riaprendo l'app/browser entra diretto (niente form vuoto).
4. La vecchia UI non ricompare dopo l'aggiornamento del server: `no-cache` dell'handler (Task 3) — aggiornamento server = UI nuova al successivo lancio.
5. Tastiera: tab visita bottoni con anello di focus; Enter su Accedi (form onsubmit=false non invia pagina).
6. Niente rottura col desktop: app Windows mostra identica al browser (finestra nativa invariata).
