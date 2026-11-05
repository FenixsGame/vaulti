# Vaulti — Spec MVP

Data: 2026-11-05 (rev.2)
Stato: approvata
Owner: FenixsGame
Repo: https://github.com/FenixsGame/vaulti

## 1. Scopo

Cloud personale stile OneDrive, uso personale, costo zero. Server su un gateway Linux dedicato con
diversi TB, accesso da browser dai PC della tailnet (Tailscale), dashboard web. MVP senza client
desktop, senza backup.

## 2. Decisioni

- Stack: Rust + axum 0.8 + tokio + sqlx 0.8 (SQLite, feature `macros` + `migrate`). Dashboard
  statica HTML/JS, niente framework, niente build.
- Accesso solo dentro la tailnet Tailscale; HTTPS con `tailscale serve`. Niente Caddy.
- Password: argon2id (crate `argon2`, versione `0.5`); hash e verify dentro `spawn_blocking`
  così il runtime async non si blocca. Lunghezza minima password: 8.
- Sessioni: token casuale 256 bit. Nel DB si salva solo lo **hash SHA-256** del token (crate
  `sha2`): dal DB non si risale al cookie. Cookie: `session=<token>; HttpOnly; Secure;
  SameSite=Lax; Path=/; Max-Age=2592000` (30 giorni).
- Rate limit login: contatore in memoria. **Due chiavi**: IP = **ultimo elemento** di
  `X-Forwarded-For` (quello aggiunto dal proxy fidato; il primo è falsificabile) e
  `mail:<email>`. Il login risponde 429 se **una delle due** è in attesa
  (`check(ip).and(check(email))`, non `or`). Regola: dopo 5 falliti, attesa `2^(n-5)`
  secondi con limite 60; `last_fail` aggiornato a ogni fallito; reset dopo 1 h senza
  falliti o al successo; a ogni login `reap()` cancella le voci vecchie (le email
  inventate si accumulano nel map). Orologio iniettabile per test senza sleep.
  Accettato: chi è in tailnet può bloccare l'admin al massimo 60 secondi per volta.
- Registrazione chiusa e atomica: un solo statement
  `INSERT INTO users(...) SELECT ... WHERE (SELECT COUNT(*) FROM users) = 0` — rows_affected 0
  → 403. Niente race tra due richieste. Primo utente = admin. Subito dopo il deploy chiunque
  nella tailnet può diventare admin: fare la registrazione appena il servizio parte.
- Secondo utente: `POST /api/admin/users {email,password}` admin-only. Niente codici invito.
- Upload: un solo file temporaneo preallocato. `start` crea `tmp/<upload_id>/data` con
  `set_len(size)`. Ogni chunk viene scritto in streaming (non bufferizzato in memoria) a offset
  `n × chunk_size`. `finish` verifica bytes ricevuti == size e poi fa `rename` verso
  `storage/<user_id>/<uuid>`. Zero copie di dati, idempotente (riscrivere lo stesso chunk
  riscrive i stessi offset).
- Casi limite upload: file da **0 byte** → `total_chunks = 0`, `finish` con zero righe
  `upload_chunks` accettato (somma 0 == size). La riga `upload_chunks` viene inserita **solo
  dopo** la verifica della lunghezza esatta del chunk: un chunk interrotto non risulta mai
  ricevuto. Prima del `rename` finale: `sync_all` del file. Lo sweep di `start` cancella
  directory + righe di `uploads` e di `upload_chunks` (**tutte**, non solo quelle vecchie
  quell'upload).
- Validazione upload: l'upload_id è un UUID generato dal server; il chunk non tocca il disco
  prima di aver verificato che l'upload esista e appartenga alla sessione; `n < total_chunks`;
  lunghezza del body limitata (`DefaultBodyLimit` sulla sola route chunk). Ogni riga
  `upload_chunks` porta la sua `len` reale. Atteso: tutti i chunk `== chunk_size`, l'ultimo
  `== size - (total_chunks-1) × chunk_size` (mai 0). `files.size` = somma reale verificata a
  finish, non la size dichiarata dal client.
- Download Range RFC 9110: end oltre la fine → troncato, 206; start ≥ size → 416 con
  `Content-Range: bytes */<size>`; gestiti `bytes=s-e` e suffix `bytes=-N`. Implementazione con
  `tower_http::services::ServeFile` (già testata), chiamata dall'handler dopo l'auth.
- XSS dai file caricati: la download route risponde sempre con `Content-Disposition: attachment`
  (filename ASCII + `filename*=UTF-8''...` per non-ASCII), `X-Content-Type-Options: nosniff`,
  `Content-Security-Policy: default-src 'none'`. Un HTML caricato non gira mai inline.
- Cancel e abbandonati: `DELETE /api/upload/:id` (solo owner) rimuove righe + tmp; ogni `start`
  fa sweep dei `tmp/*` più vecchi di 7 giorni.
- Spazio: `start` risponde 507 se `available_space(data_dir) < size` (crate `fs4`).
- Cartelle nel modello da subito (niente migrazioni dopo): `folders(id, user_id,
  parent_id NULL, name, created_at)` + `files.folder_id NULL` (NULL = radice). Endpoint:
  `POST /api/folders`, `PATCH /api/folders/:id`, `DELETE /api/folders/:id` (solo se vuota:
  file o subfolder dentro → 409), `PATCH /api/files/:id` (rinomina/sposta), `GET
  /api/files?folder_id=`. Anti-ciclo allo spostamento: il nuovo parent non può essere la
  cartella stessa o un suo discendente (risalita di `parent_id`). Nomi duplicati vietati per
  **cartelle e file**: `UNIQUE(user_id, parent_key, name)` con `parent_key` colonna generata
  `COALESCE(parent_id, '!root!')`, e `UNIQUE(user_id, folder_key, name)` su `files` con
  `folder_key` = `COALESCE(folder_id, '!root!')`; duplicato → 409 "nome già in uso" (anche a
  `finish` per i file).
- Delete: **hard delete** (DELETE riga + `remove_file` blob). Scelta consapevole: senza backup
  l'hard delete è irreversibile; il soft delete senza purge non libererebbe mai spazio.
  Cestino + purge = fase B.
- SQLite: WAL + busy_timeout 5 s; `users.email UNIQUE COLLATE NOCASE`.
- CSRF: le route che modificano dati (POST/PUT/PATCH/DELETE) verifichano **Sec-Fetch-Site**:
  ammessi solo `same-origin` e `none`. Ripiego quando l'header manca (browser vecchi): header
  `Origin` assente → ok, presente → confronto l'host con `X-Forwarded-Host` (e con `Host`);
  mismatch → 403. Da verificare con la realtà di `tailscale serve` sul gateway, il che
  avverrà durante il deploy: se Host viene a 127.0.0.1, la regola di confronto Host
  bloccherebbe tutta la dashboard.
- Cambio password di emergenza: subcommand CLI `vaulti-server reset-password <email>`, la nuova
  password letta **da stdin** (mai in argv, resterebbe nella history della shell). Aggiorna
  hash argon2 + invalida tutte le sessioni di quell'utente.
- Filesystem del disco dati: deve essere **ext4, XFS o btrfs locale** (mai NTFS/exFAT/SMB:
  WAL e rename atomico ne risentono). `tmp/` e `storage/` vivono sotto la stessa `VAULTI_DATA_DIR`
  (stessa mount point). Verifica nel deploy README.
- `files.deleted_at` resta nello schema per la fase B (cestino con purge); l'MVP non lo usa.
- Anteprime inline di file (immagini/video/PDF): fase B, route separata con allowlist di mime
  (mai HTML/SVG). Il download attuale attachment-only copre l'MVP.
- Fuori dall'MVP: trash/cestino, versioni dei file, 2FA, condivisione, quote per utente, backup
  (rischio noto e accettato: disco muore = file muoiono), app mobile, sync automatico.
- Build: GitHub Actions (rustup installato sul PC con l'assistente per girare i test in locale);
  nessun Rust/Node sul gateway: binario statico target `x86_64-unknown-linux-musl`, niente
  problemi di versioni glibc.

## 3. Architettura

```
[PC Windows] e [altri PC]  --tailnet Tailscale-->  gateway Linux (Debian/Ubuntu)
                                                   systemd: vaulti.service
                                                     axum 0.8 → 127.0.0.1:8420
/srv/vaulti/ (override: VAULTI_DATA_DIR):
  vaulti.db                    SQLite WAL, busy_timeout 5 s
  storage/<user_id>/<uuid>     blob finali
  tmp/<upload_id>/data         preallocato, in corso di upload
```

- `tailscale serve https / proxy http://127.0.0.1:8420` fornisce HTTPS dentro la tailnet.
- La dashboard è servita da axum stesso: `include_str!` dei 3 file.
- Percorsi: il **default nel codice** è `/srv/vaulti` (override `VAULTI_DATA_DIR`); il deploy
  usa il disco dati reale, quindi il systemd unit imposta `VAULTI_DATA_DIR=/mnt/data/vaulti`.
  Vale il §2 sul filesystem (ext4/XFS/btrfs, tmp e storage stessa mount).

## 4. Modello dati

```sql
users    (id TEXT PK, email TEXT NOT NULL UNIQUE COLLATE NOCASE, password_hash TEXT NOT NULL,
          is_admin INTEGER NOT NULL, created_at INTEGER NOT NULL)
sessions (token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id),
          expires_at INTEGER NOT NULL)
folders  (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id),
          parent_id TEXT REFERENCES folders(id), name TEXT NOT NULL, created_at INTEGER NOT NULL)
files    (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id),
          folder_id TEXT REFERENCES folders(id), name TEXT NOT NULL, size INTEGER NOT NULL,
          mime TEXT NOT NULL, stored_name TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL,
          deleted_at INTEGER)
uploads  (id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), name TEXT NOT NULL,
          chunk_size INTEGER NOT NULL, total_chunks INTEGER NOT NULL, size INTEGER NOT NULL,
          created_at INTEGER NOT NULL)
upload_chunks (upload_id TEXT NOT NULL REFERENCES uploads(id), chunk_n INTEGER NOT NULL,
          len INTEGER NOT NULL, received_at INTEGER NOT NULL,
          PRIMARY KEY (upload_id, chunk_n))
```

## 5. API

| Metodo+route | Comportamento |
|---|---|
| `POST /auth/register` | valida `users` vuoto nell'INSERT; password min 8; crea admin |
| `POST /auth/login` | rate limit XFF; login; pulisce le sessioni scadute |
| `POST /auth/logout` | invalida il token |
| `GET /api/me` | `{email, is_admin}` (bool) |
| `POST /api/admin/users` | admin-only: crea un altro utente |
| `GET /api/files?folder_id=` | file + sottocartelle della cartella; quota `{used, free}` |
| `POST /api/folders` | `{name, parent_id?}` → `{"id"}` |
| `PATCH /api/folders/:id` | `{name?, parent_id?}` rinoma/sposta (anti-ciclo) |
| `DELETE /api/folders/:id` | solo se vuota (file o subfolder dentro → 409) |
| `PATCH /api/files/:id` | `{name?, folder_id?}` rinoma/sposta |
| `POST /api/upload/start` | `{name,size}`; check spazio; preallocazione → `{"upload_id","chunk_size"}` |
| `PUT /api/upload/:id/chunk/:n` | body stream su tmp a offset; valida owner+n+len; 204 |
| `GET /api/upload/:id/status` | `{"received":[n...]}` |
| `POST /api/upload/:id/finish` | somma len == size → rename + inserimento `files` → `{"id"}`; altrimenti 409 |
| `DELETE /api/upload/:id` | annulla e pulisce tmp |
| `GET /api/files/:id/download` | auth; ServeFile Range; header anti-XSS |
| `DELETE /api/files/:id` | hard delete (riga + blob) |

Errori JSON `{error}`: 401 sessione, 403, 409, 416 range, 507 spazio.

## 6. Sicurezza

- argon2id (hash e verify in `spawn_blocking`, costi default).
- Token sessione: solo l'hash sha256 sta nel DB.
- Cookie `HttpOnly; Secure; SameSite=Lax`.
- Rate limit XFF come in §2.
- Path: solo UUID generati dal server; il `name` del client non è mai usato come path.
- Download: `Content-Disposition: attachment` + nosniff + CSP restrittiva.
- Query parametrizzate ovunque (bind sqlx).
- Ogni query file/folder ha implicito il filtro `user_id` della sessione.
- Hard delete consapevole; cestino con purge = fase B.

## 7. Build e deployment

- `rust-toolchain.toml`: channel `1.99.1` (esiste: rustc locale la usa) con
  `components = ["clippy", "rustfmt"]`, `targets = ["x86_64-unknown-linux-musl"]`.
- `.github/workflows/ci.yml` su push (main) e su `tags: ['v*']`:
  `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`.
- Tag `v*` → job release: `cargo build --release --target x86_64-unknown-linux-musl`
  (runner `musl-tools`), copia con nome fisso `vaulti-server-linux-musl` e upload su
  GitHub Releases; il curl del README usa esattamente quello stesso nome.
- Gateway: useradd `vaulti`, dati in `/mnt/data/vaulti` (+ conf systemd in `deploy/vaulti.service`),
  download binario da Releases, `tailscale serve --bg --https=443 http://127.0.0.1:8420`,
  prima registrazione browser → admin.

## 8. Testing

- Unit: argon2 hash/verify, RateLimiter (last_fail aggiornato), session token hash, mime da ext.
- Integrazione (oneshot, `tempfile` per data dir, `chunk_size=2` configurabile): register→403;
  login/me/logout; scadenza sessione; 429 rate limit; upload ordine/idempotenza/409/cancel/507;
  PATCH rename/move; download 200/206/416/404; cross-user 404; header anti-XSS; smoke `/`.
- e2e reale nel deploy: curl + browser (tailnet).

## 9. Fasi

1. Server MVP (Task 1-5 del piano) 2. Dashboard (Task 6) 3. Deploy gateway (Task 7) 4. Fase B.
