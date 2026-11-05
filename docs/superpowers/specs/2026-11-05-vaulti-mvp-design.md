# Vaulti — Spec MVP

Date: 2026-11-05
Status: draft for review
Owner: FenixsGame
Repo: https://github.com/FenixsGame/vaulti

## 1. Scopo

Cloud personale stile OneDrive, uso personale, costo zero. Server su un gateway Linux dedicato con
diversi TB, accesso da browser dai PC della tailnet (Tailscale), dashboard web. MVP senza client
desktop, senza backup, senza condivisione.

## 2. Decisioni già prese (con lo stakeholder)

- Stack: Rust + axum + tokio + sqlx(SQLite). Dashboard statica HTML/JS (no framework, no build).
- Accesso: solo dentro la tailnet Tailscale. HTTPS via `tailscale serve`. Niente Caddy.
- Autenticazione: password con argon2id; sessioni in tabella `sessions` (token casuale 256 bit,
  cookie HttpOnly + Secure). Niente JWT/refresh — uso personale, 1-2 utenti.
- Registrazione chiusa: il primo utente creato via `/auth/register` diventa admin; finché `users`
  è non vuoto l'endpoint risponde 403. Codici invito in fase successiva.
- Upload a blocchi per riprendere i file grandi; protocollo proprietario semplice, non tus.
- Download con supporto `Range` per lo streaming e ripresa dei download.
- Utente non ha esperienza Rust: tutto il codice scritto dall'assistente; build e test su GitHub
  Actions; nessun toolchain Rust/Node sul PC dello stakeholder.
- Fuori dal MVP (non-scopi): trash/cestino, versioni dei file, 2FA, condivisione con scadenza,
  quota per utente admin, backup (rischio accettato e noto: disco che muore = file che muoiono),
  app mobile, sync automatico delle cartelle.

## 3. Architettura

```
[PC Windows] e [altri PC]  --Tailscale (tailnet)-->  gateway Linux (Debian/Ubuntu)
                                                     systemd: vaulti.service
                                                       axum 0.8 → 127.0.0.1:8420
Dati in /srv/vaulti:
  vaulti.db                  ← SQLite (sqlx)
  storage/<user_id>/<uuid>   ← blob file, nome su disco = UUID
```

- `tailscale serve https / proxy http://127.0.0.1:8420` fornisce HTTPS valido dentro la tailnet.
- La dashboard (`index.html`, `app.js`, `style.css`) è servita dall'axum stesso dal modulo statico.

## 4. Modello dati

```sql
users    (id TEXT pk/uuid, email TEXT unique, password_hash TEXT, is_admin BOOL, created_at INT)
sessions (token TEXT pk, user_id TEXT fk, expires_at INT)          -- scadenza 30 giorni, pulita a login
files    (id TEXT pk, user_id TEXT fk, name TEXT, size INT, mime TEXT,
          stored_name TEXT unique, created_at INT, deleted_at INT NULL)
upload_chunks (upload_id TEXT pk, user_id TEXT fk, name TEXT,
               chunk_n INT pk composto, total_chunks INT, total_size INT, updated_at INT)
```

- `files.deleted_at IS NULL` = attivi. Soft delete (fisicalmente rimossa in fase cestino).

## 5. API

Tutte le route `/api/*` richiedono sessione valida ad eccezione di auth.

| Metodo+route                          | Comportamento |
|---|---|
| `POST /auth/register`                 | 403 se esiste almeno un utente; altrimenti crea admin |
| `POST /auth/login`                    | email+password → cookie sessione; rate limit per IP |
| `POST /auth/logout`                   | invalida la sessione |
| `GET  /api/me`                        | `{email, is_admin}` |
| `GET  /api/files?deleted=false`       | elenco file dell'utente + quota usata |
| `POST /api/upload/start`              | `{name,size}` → `{upload_id, chunk_size: 8 MiB}` |
| `PUT  /api/upload/:id/chunk/:n`       | corpo = n blob; idempotente (sovrascrive lo stesso n) |
| `POST /api/upload/:id/finish`         | verifica presenza blocchi 0..total-1, concatena su disco, crea la riga in `files`, cancella i chunk |
| `GET  /api/upload/:id/status`         | elenco degli `n` già ricevuti — serve al client per riprendere un upload interrotto |
| `GET  /api/files/:id/download`        | streaming con `Range` (206 / 416), `Content-Length` |
| `DELETE /api/files/:id`               | soft delete `deleted_at` |

Errori: JSON `{error: "…"}` con stato HTTP corretto. Upload con sessione scaduta → 401; il client
riprende con `GET /api/upload/:id/status` e ri-manda solo i blocchi mancanti.

## 6. Sicurezza

- Password: argon2id, salt utile, costi di default del crate `argon2`.
- Sessione: token casuale 256 bit (rand di `getrandom`), cookie `HttpOnly; Secure; SameSite=Lax; Path=/`.
- Rate limit login: contatore in memoria per IP (minimo 5 errori → attesa esponenziale); reset al successo.
- Traversing: `stored_name` è sempre un UUID generato dal server; mai usare il `name` del client
  per accedere al disco. Ogni query ha `WHERE user_id = session.user_id`.
- Scratch dir per chunk: `/srv/vaulti/tmp/<upload_id>/part_<n>`; la directory vive sotto
  `/srv/vaulti`, stessa partizione — la concatenazione finale è `rename`, 0 fallback.
- HTTPS di rete gestito da `tailscale serve`; il processo axum resta su `127.0.0.1`.

## 7. Build e deployment

- Il codice vive nel repo `FenixsGame/vaulti`; GitHub Actions su ogni push esegue `cargo fmt --check`,
  `cargo clippy -D warnings` e `cargo test` (runner ubuntu).
- Tag `v*` → release con binario Linux x86_64 (`cargo build --release`), il gateway lo scarica.
- Nessun toolchain Rust sul gateway: si scarica il binario precompilato (Linux x86_64 GNU; glibc
  già presente su Debian/Ubuntu).
- Prima installazione gateway: binario scaricato + `vaulti.service` systemd (Restart=always,
  User=vaulti, StateDirectory) + Tailscale installato + `tailscale serve`.

## 8. Testing

- Unit test la logica di auth (hash/verify, scadenza sessione) e della sequenza chunk/finish
  (blocchi spaiati, rispettivo ordine, doppio finish).
- Integration test con `tower::ServiceExt::oneshot` (axum testing utils): register/login/upload/
  download con `Range`, cross-user 404, sessione scaduta 401.
- Il test del deploy reale è manuale: curl con una coda di upload da dentro la tailnet.

## 9. Roadmap fasi

1. **Server MVP** — crate `server`, migrazioni sqlx, tutte le route API della tabella §5, test §8
2. **Dashboard web** — login, elenco, drag&drop con progresso per chunk, azioni di download/delete, quota
3. **Deploy gateway** — systemd + tailscale serve + prima registrazione reale
4. **Fase B (dopo l'uso reale)** — client Windows Tauri + tray, cestino, condivisioni con scadenza/password,
   inviti, 2FA TOTP admin, backup

MVP = fasi 1+2+3.
