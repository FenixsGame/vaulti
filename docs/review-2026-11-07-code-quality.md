# Vaulti — Thermo-Nuclear Code Quality Review

Data: 2026-11-07
Perimetro: `server/src`, `server/tests`, `web/`, `desktop/src-tauri/src`, `desktop/ui`, `.github/workflows`
Metodo: lettura integrale di tutto il codice di prima parte (≈1.400 righe Rust + 280 JS + 115 CSS, test esclusi), grep mirati su duplicazioni e dead code, verifica dei test JS.
Nessuna modifica al codice applicativo: questo file è solo la review.

---

## Verdetto

**Non approvo lo stato attuale come base su cui continuare ad aggiungere feature.**

Il comportamento è, per quanto leggibile, corretto e i test sono insolitamente buoni per un MVP (upload a chunk, range, owner-check, anti-ciclo cartelle). Il problema non è la correttezza: è che **la struttura ha accumulato complessità incidentale che ora costa più di quanto costasse scriverla bene**, e ogni feature nuova la paga.

Tre sono i punti che rendono il "no" non negoziabile:

1. **Non esiste un tipo di errore.** Ogni handler restituisce `Response`, gestisce i fallimenti con 67 `unwrap()`, e ricostruisce a mano lo stesso JSON `{"error": ...}` in tre file. Le firme non dicono nulla su cosa può fallire, un errore DB/FK uccide la connessione invece di produrre un 500, e i "409 se unique violation" sono copiati 5 volte.
2. **La stessa informazione è rappresentata due o tre volte**: path come `String` concatenate a mano in ~14 punti, lo stato di un upload come tupla `(i64, i64, i64, String)` indicizzata `.0/.1/.2/.3`, la posizione corrente nella UI come `folderId` *e* `crumbPath`, e "cartella radice" come `NULL` *e* `""`.
3. **`server/tests/integration.rs` è un file unico da 1.275 righe** con helper duplicati rispetto a `tests/common`. Ha già sfondato la soglia dei 1.000: è il posto dove la decomposizione è più facile e più redditizia.

Nessuno di questi richiede un rewrite. Sono quattro-cinque rifattorizzazioni locali che **tolgono** codice: il piano in fondo stima ~400 righe nette in meno a parità di comportamento. Fino a quando non sono fatte, ogni nuova funzione (cestino, ricerca, condivisione, admin UI, sync) aggiunge un altro giro di `format!`, un altro `unwrap`, un altro `err()` e un altro test dentro il file da 1.275 righe.

---

## 1. Bloccanti strutturali

### B1. Nessun tipo di errore → `err()` ×3, `now()` ×3, 67 `unwrap()`

`server/src/handlers_auth.rs:19`, `handlers_files.rs:11`, `handlers_store.rs:14` definiscono **la stessa** funzione `err(status, msg)`. Idem `now()` (`handlers_auth.rs:12`, `handlers_files.rs:15`, `handlers_store.rs:18`). Idem `is_unique_violation → 409` in 5 punti (`handlers_auth.rs:197`, `handlers_files.rs:136,192,264`, `handlers_store.rs:251`).

Conta degli `unwrap()` su codice di prima parte:

| file | `unwrap()` |
|---|---|
| `server/src/handlers_store.rs` | 25 |
| `server/src/handlers_files.rs` | 15 |
| `server/src/lib.rs` | 12 |
| `server/src/handlers_auth.rs` | 8 |
| `server/src/auth.rs` | 7 |
| `desktop/src-tauri/src/config.rs` | 7 |

Questi non sono "unwrap in un test": sono `sqlx::query(...).await.unwrap()` e `tokio::fs::...unwrap()` dentro handler HTTP. Un `SQLITE_BUSY` dopo 5 s di `busy_timeout`, un disco pieno a metà upload, una FK violata in una race su `delete_folder` → panico nel task, connessione chiusa, **nessuna risposta HTTP**, nessun log leggibile.

Il rimedio è uno e uno solo, ed è la mossa di code-judo più grande disponibile:

```rust
pub enum AppError { NotFound(&'static str), Conflict(&'static str), BadRequest(String), Db(sqlx::Error), Io(std::io::Error), Unauthorized }
impl IntoResponse for AppError { /* un solo punto che mappa su status + {"error": ...} */ }
impl From<sqlx::Error> for AppError { /* unique violation → 409, resto → 500 + log */ }
impl From<std::io::Error> for AppError { /* NotFound su ENOENT, resto → 500 */ }
```

Con questo, `sqlx::query(...).await?` sostituisce ogni `.unwrap()` e ogni blocco `if e.as_database_error().map(...is_unique_violation)...`, le tre `err()` spariscono, e tutte le firme diventano `Result<Json<T>, AppError>`. È il singolo cambiamento che rende leggibili tutti e tre i file di handler: da ~890 righe a ~600, con la stessa semantica HTTP.

Nota: `AppError` deve anche dare un `Result` a `SessionUser` con un rejection che produce lo stesso JSON (oggi `handlers_auth.rs:26` restituisce `StatusCode` nudo, quindi un 401 ha body vuoto e la UI mostra `"401"` come stringa — vedi `web/js/api.js:7`).

### B2. Path del filesystem costruiti a mano, 14+ volte

`AppState.data_dir` è `Arc<String>`, e ogni punto fa `format!("{}/storage/{}/{}", s.data_dir, uid, stored)` oppure `format!("{}/tmp/{id}", s.data_dir)`:

`lib.rs:108-111`, `handlers_files.rs:295,313`, `handlers_store.rs:48,79,129,221,224,262,281,291,317`.

Questo è il motivo per cui esistono i commenti difensivi tipo *"l'id è un UUID generato dal server: non è mai un percorso"* (`handlers_store.rs:104`) e *"il path è derivato da stored_name di proprietà confermata"* (`handlers_files.rs:311`). La difesa è giusta ma è **implicita e sparsa**: chi aggiunge il prossimo handler deve ricordarsela.

Code-judo: spostare la conoscenza dei path dentro `AppState`, una volta sola:

```rust
impl AppState {
    fn root(&self) -> &Path;              // data_dir: Arc<PathBuf>
    fn storage_dir(&self, user: &UserId) -> PathBuf;
    fn blob(&self, user: &UserId, stored: &StoredName) -> PathBuf;
    fn upload_dir(&self, id: &UploadId) -> PathBuf;  // tmp/{id}
}
```

I 14 `format!` diventano 0, i due commenti difensivi diventano un tipo (`UploadId`, `StoredName` costruiti solo da UUID server-side) e i test possono passare `Path` invece di `String` lossy (`tests/common/mod.rs:21`).

### B3. Il "tri-state" `NULL` / stringa vuota come protocollo implicito

Il contratto PATCH è: `field` assente = non toccare, `null` = non toccare, `""` = imposta a radice, valore = imposta. Vive in due SQL quasi identici (`handlers_files.rs:175-181` per le cartelle, `handlers_files.rs:247-254` per i file):

```sql
UPDATE ... SET name = COALESCE(?1, name),
  parent_id = CASE WHEN ?2 IS NULL THEN parent_id WHEN ?2 = '' THEN NULL ELSE ?2 END
```

e nel client come stringa magica: `body: JSON.stringify({ folder_id: sel.value })` dove `""` significa "Home" (`web/js/app.js:105`, commento alla riga 100), con l'`<option value="">Home</option>` costruito a mano (`app.js:50-53`).

Questo è esattamente il tipo di "magic handling che nasconde una struttura semplice" da eliminare. Rimedio: un enum esplicito nel body, decodificato una volta:

```rust
#[derive(Deserialize)] #[serde(untagged)]
enum Patch<T> { Keep, Clear, Set(T) }   // assente | null | valore
```

e **una** funzione `apply_patch(pool, table, id, user, name: Patch<String>, parent: Patch<FolderId>)` condivisa da file e cartelle. Spariscono: due query quasi uguali, i due `CASE WHEN ... ''`, il `if !pid.is_empty()` (`handlers_files.rs:156`), il `if !fid.is_empty()` (`handlers_files.rs:235`) e il commento che spiega la convenzione alla riga 100 del client. Il client manda `folder_id: null` per la radice, che è ciò che significa.

### B4. Lo stato di un upload è una tupla anonima di 4 elementi

`handlers_store.rs:25` (`uploads_row`) restituisce `Option<(i64, i64, i64, String)>` = `(chunk_size, total_chunks, size, name)`. Il chiamante poi fa aritmetica su indici nudi:

```rust
if n < 0 || n as i64 >= up.1 { ... }              // :117  "up.1" è total_chunks
let expected = if n as i64 + 1 == up.1 { up.2 - (up.1 - 1) * up.0 } else { up.0 };  // :121-124
if got != up.1 || sum != up.2 { ... }             // :213
```

Chi legge deve ricostruire la mappa dei campi ogni volta, e un refactor dell'ordine della SELECT compila e sbaglia in silenzio. `#[derive(sqlx::FromRow)] struct Upload { chunk_size: i64, total_chunks: i64, size: i64, name: String }` con un metodo `expected_len(&self, n: i64) -> i64` elimina l'aritmetica duplicata e rende `chunk` leggibile in 5 righe. Stessa cosa per `list` (`handlers_files.rs:73-98`, due tuple a 2 e 5 elementi mappate a mano in `serde_json::json!`): due `struct` con `#[derive(Serialize)]` tolgono 15 righe di mapping manuale.

### B5. `files.deleted_at` è una soft-delete morta

La cancellazione è **hard** (`handlers_files.rs:288`: `DELETE FROM files`, e `deploy/README.md` lo dice esplicitamente: "non esiste cestino"). Eppure `deleted_at` sopravvive in 7 punti come `AND deleted_at IS NULL` (`handlers_files.rs:78,86,216,280,308`, più l'indice in `migrations/0001_init.sql:43`) e nella `UNIQUE(user_id, folder_key, name)`: un file cancellato non ha più riga, quindi il predicato è sempre vero e l'indice è inutile.

È costo puro: 5 predicati da ricordare in ogni query nuova, un indice da mantenere, e un modello mentale ("forse c'è un cestino?") che non esiste. Rimedio: una migrazione `0002_drop_soft_delete.sql` (SQLite ≥3.35 supporta `ALTER TABLE ... DROP COLUMN`) e 5 righe di codice in meno. Se il cestino è in roadmap, allora va **implementato**, non lasciato a metà: la colonna da sola non lo è.

---

## 2. Duplicazioni che vanno cancellate, non riordinate

### D1. La pulizia di un upload esiste in 4 copie

La sequenza *delete `upload_chunks` → delete `uploads` → `remove_dir_all("tmp/{id}")`* è scritta quattro volte con varianti:

- sweep all'inizio di `start` (`handlers_store.rs:58-68`)
- conflitto nome in `finish` (`handlers_store.rs:255-270`)
- successo in `finish` (`handlers_store.rs:283-295`)
- `cancel` (`handlers_store.rs:306-318`)

Quattro copie della stessa invariante, con `unwrap()`/`.ok()`/`let _ =` diversi in ognuna: è così che nascono i blob orfani. Una sola `async fn discard_upload(pool, dirs, id)` con una transazione per le due DELETE. `fresh`/`cancel` diventano una riga.

Bonus collegato: lo sweep gira **nel percorso critico di ogni upload** (`start`), in serie, con 2 query + `remove_dir_all` per ogni upload scaduto. Spostarlo in un task periodico all'avvio (`tokio::spawn` + intervallo) toglie latenza variabile dal primo chunk e semplifica `start` di 15 righe.

### D2. `mime` è definito due volte, in due posti sbagliati

- `auth::mime_of` (`server/src/auth.rs:106`) — un mapping estensione→MIME dentro il modulo di autenticazione, per nessuna ragione.
- `handlers_static::mime` (`server/src/handlers_static.rs:16`) — un secondo mapping, con valori leggermente diversi (`text/javascript` vs niente, `text/html; charset` vs no).

Due tabelle che descrivono la stessa cosa e divergeranno al primo formato nuovo. Rimedio: un modulo `mime` con una sola tabella e due funzioni (`for_upload(name)`, `for_asset(path)`), oppure `mime_guess` se si accetta la dipendenza. `auth.rs` smette di essere il posto in cui si parla di estensioni dei file.

### D3. Nel desktop, il flusso "connetti" è scritto tre volte

`lib.rs:67-84` (setup), `lib.rs:179-197` (`save_and_connect`), `lib.rs:199-209` (`retry_connect`): tutti fanno *leggi config → healthz → `set_server` → `navigate`*, con gestione errori leggermente diversa. `window_of` + `show` + `set_focus` è duplicato in `activate` (`lib.rs:216`) e inline nel plugin single-instance (`lib.rs:54-58`).

Una `async fn connect(app, cfg) -> Result<(), String>` cancellata tre volte, e `activate` usata dal single-instance. Inoltre la config viene letta **tre volte** per singola esecuzione (setup, `spawn_retry_loop:225`, `save_and_connect:183`): va letta una volta e messa in `manage`.

### D4. Nel frontend, l'header JSON è copiato 6 volte

`web/js/app.js:72,96,105,145,185,202,231`: `headers: { "content-type": "application/json" }`. `api()` (`web/js/api.js:4`) potrebbe impostarlo da sé quando c'è un body, e accettare un oggetto invece di `JSON.stringify` a mano. Le sei `fetch` diventano `api(url, { method: "PATCH", body: { name: n } })`. Sono ~20 righe e un'intera classe di errori (dimenticare l'header) che scompare.

### D5. Le due pagine del guscio desktop sono la stessa pagina

`desktop/ui/index.html` e `desktop/ui/error.html` duplicano **identici**: blocco `:root/body/main/button/#msg/h1` di CSS, e il bootstrap `const invoke = window.__TAURI_INTERNALS__.invoke`. In un "guscio sottile" da 71 righe totali la duplicazione è ~50%. Estrarre `ui/shell.css` + `ui/ipc.js` è banale e rende le due pagine quello che dovrebbero essere: 15 righe di contenuto ciascuna.

---

## 3. Spaghetti e controlli di flusso impliciti

### S1. `owns(pool, user_id, table: &str, id)` costruisce SQL per nome di tabella

`handlers_files.rs:23-31`:

```rust
let q = format!("SELECT 1 FROM {table} WHERE id = ? AND user_id = ? LIMIT 1");
```

Oggi i chiamanti passano solo `"folders"` e `"files"`, quindi non è una vulnerabilità — ma è un *generic mechanism che nasconde un'assunzione semplice*, ed è la forma esatta che diventa injection quando qualcuno, un anno dopo, passa un valore non letterale. Inoltre la funzione si chiama `owns` e restituisce `bool`: un `false` significa sia "non esiste" sia "non è tuo", e i chiamanti devono ricostruire il messaggio giusto ogni volta (`"cartella sconosciuta"`, `"file sconosciuto"`, `"cartella padre sconosciuta"`...).

Rimedio: due funzioni tipizzate `folder_of(pool, &UserId, &FolderId) -> Result<(), AppError>` e `file_of(...)`, oppure un enum `Resource::{Folder, File}` con `fn table(self) -> &'static str`. Il nome della tabella diventa una costante del compilatore, e `NotFound` porta già il messaggio.

### S2. `origin_guard` ha un fallback che si auto-avvalora

`lib.rs:68-98`. Se manca `Sec-Fetch-Site`, il confronto è `Origin`-host contro `X-Forwarded-Host` **con precedenza su `Host`** (righe 84-86). Se il proxy a monte inoltra un `X-Forwarded-Host` fornito dal client, un attaccante cross-site manda `Origin: https://evil.com` **e** `X-Forwarded-Host: evil.com`, i due coincidono, e la richiesta passa. È una difesa CSRF che può essere soddisfatta da chi attacca, e il commento stesso ammette *"Da verificare sul gateway con tailscale serve"*.

Peggio: **questa è la parte di sicurezza più delicata del server e non ha un solo test.** `integration.rs` non contiene nessuna richiesta con `Origin` o `Sec-Fetch-Site`.

Rimedio: un'unica fonte di verità esplicita (un `allowed_origin` dal config, oppure confronto con `Host` e *mai* con `X-Forwarded-Host`), più 4 test: `Sec-Fetch-Site: cross-site` → 403; `Origin` estraneo → 403; `Origin` uguale → 200; nessun header → comportamento documentato. La regola "se non so chi sei, rifiuto" è più semplice di due ripieghi.

### S3. `delete_folder` fa check-then-act senza transazione

`handlers_files.rs:206-227`: conta figli cartelle, conta file, poi `DELETE`. Tra il conteggio e la DELETE c'è una finestra in cui un altro tab può creare un figlio: la FK `parent_id REFERENCES folders(id)` senza `ON DELETE` fa fallire la DELETE, `.unwrap()` (`:225`) manda in panico l'handler, e il client riceve una connessione chiusa invece di 409. O si fa tutto in una transazione (`BEGIN IMMEDIATE`, conteggi, delete), o si fa la DELETE e si mappa l'errore FK su 409 — che è esattamente ciò che `AppError::from(sqlx::Error)` (B1) dà gratis.

### S4. `patch_folder` risale i parent con un N+1

`handlers_files.rs:161-174`: un `SELECT parent_id` per ogni livello dell'albero, in un loop, per verificare i cicli. Con un albero profondo sono N round-trip per un singolo PATCH, ed è logica che appartiene al DB. Una recursive CTE risolve in una query:

```sql
WITH RECURSIVE up(id) AS (SELECT ?1 UNION ALL SELECT f.parent_id FROM folders f JOIN up ON f.id = up.id WHERE f.parent_id IS NOT NULL)
SELECT 1 FROM up WHERE id = ?2
```

Una query, un booleano, niente `match nxt { Some(Some(c)) => ... }`.

### S5. `register` calcola l'hash Argon2 **prima** di sapere che la registrazione è chiusa

`handlers_auth.rs:78-100`: valida la lunghezza, poi `auth::hash(...)` (Argon2, deliberatamente costoso, `spawn_blocking`), poi `INSERT ... WHERE (SELECT COUNT(*) FROM users) = 0`. Dopo il primo utente, **ogni** richiesta anonima a `/auth/register` brucia un hash Argon2 completo e un thread del blocking pool per finire in 403. Non c'è rate limit su `register` (il limiter è solo in `login`). Chi è nella tailnet — e il README dice che chiunque nella tailnet può arrivare — può saturare il pool con una riga di curl.

Fix: un `SELECT COUNT(*)` (o meglio `SELECT 1 FROM users LIMIT 1`) **prima** dell'hash, e 403 immediato. Due righe, un ordine diverso. Vale anche come principio: le validazioni a costo zero vanno prima di quelle a costo alto.

### S6. Il retry dell'upload ritenta errori deterministici, e contiene un ramo irraggiungibile

`web/js/app.js:156-173`: 5 tentativi con backoff anche quando il server risponde `413` o `400` (chunk troppo lungo/corto): errori che non possono migliorare ritentando lo stesso body, per un totale di 15,5 s di attesa e 5 richieste inutili. E `if (!ok) throw` alla riga 172 è **irraggiungibile**: all'ultimo tentativo il `catch` rilancia già. Il ciclo va riscritto come "ritenta solo su rete/5xx", con la condizione di uscita in un posto solo.

### S7. `spawn_retry_loop` (desktop) non fa nulla quando riesce

`desktop/src-tauri/src/lib.rs:223-238`: loop con backoff esponenziale su `/healthz`; al successo fa `break`. Non naviga la finestra, non ricarica la pagina d'errore, non emette eventi. La funzione quindi non serve a niente: l'utente che avvia il PC prima di Tailscale resta sulla pagina "Server irraggiungibile" finché non clicca Riprova, e questo codice non cambia nulla. O si cancella (onesto), o si trasforma in "quando healthz torna ok → `connect(app)`". Oggi è ~15 righe che danno l'illusione di una feature.

### S8. `visible(!autostart)` confonde "autostart attivo" con "avviato al boot"

`lib.rs:85`: se l'utente ha attivato l'autostart, **anche un avvio manuale dal menu Start** parte con la finestra nascosta, e l'unico modo per vederla è la tray. È un bug di comportamento nato da uno stato con due significati. La distinzione corretta è un flag di lancio (`args` del single-instance / `--hidden`), non la configurazione.

---

## 4. Confini di tipo e API

### T1. `u64`/`i64` che si incontrano a metà strada

`size: u64` nel body (`handlers_store.rs:44`) → `as i64` nel DB (`:88`) → `as u64` per `set_len` (`:83`) → `i64` in `expected` (`:121`) → confronti misti con `as`. Funziona perché i valori sono piccoli, ma il modello non ha un tipo: nel resto del server la dimensione è `i64`. Un `size: i64` validato positivo all'ingresso (o un newtype `Bytes(i64)`) toglie tutti i cast e l'invariante "mai negativo" diventa esplicita invece che sperata. Stessa cosa per `total_chunks: i64` che viene confrontato con `n: i32` (`:117`).

### T2. `u64::MAX` come valore sentinella nel JSON della quota

`handlers_files.rs:92` e `handlers_store.rs:73`: `fs4::available_space(...).unwrap_or(u64::MAX)`. Due problemi: `u64::MAX` serializzato in JSON è oltre il range sicuro di `Number` in JS (perde precisione, e la UI ci calcola sopra una percentuale), e la UI fa `tot = used + free` (`app.js:17`) — cioè **somma lo spazio usato dall'utente allo spazio libero dell'intero disco** e lo chiama "quota". Il numero mostrato ("usati 2 B su 931 GB") non è né una quota utente né l'occupazione del disco: è la somma di due grandezze di domini diversi.

Il modello giusto: `quota: { used: i64, free: i64 }` dove `free` è il disco e l'errore è `Option<i64>`; la UI deve mostrare due numeri separati, oppure il server deve introdurre una quota vera per utente (allora `free` è una costante di config). Oggi il codice finge che esista un limite che non esiste.

### T3. `available_space` (chiamata bloccante) dentro handler async

`fs4::available_space` è una `statvfs`/`GetDiskFreeSpaceEx` sincrona eseguita su un worker async: blocca il thread del runtime. Su un gateway Linux con volume di rete (`/mnt/data`) questa chiamata può prendere decine di ms, e viene fatta **a ogni listing** (`list`) e a ogni `start`. Va in `spawn_blocking`, oppure memorizzata con un TTL breve. Non è un micro-nit: è un blocco sincrono in un percorso caldo.

### T4. `RateLimiter` ha un'astrazione che non serve e si chiama male

`auth.rs:21-45`: `now: Box<dyn Fn() -> Instant + Send + Sync>` con `with_clock` "per i test". `with_clock` **non è usata da nessun test** (grep: solo la definizione). Il costo dell'indirezione è pagato per niente, e `Default::default` + `new` + `with_clock` sono tre costruttori per un tipo con un solo campo reale.

In più: `reap()` (`:47`) viene chiamata a ogni `login` e prende il lock per fare un `retain` di tutto l'HashMap — O(n) sotto mutex nel punto più conteso del server. Il commento sopra `COOKIE_MAX_AGE` (`:89-91`) spiega una semantica da `Result::and()`: se serve un commento per spiegare un `and`, quella riga va riscritta come espressione esplicita (`if let Err(w) = a { return ... }; if let Err(w) = b { ... }`).

Rimedio: togliere `with_clock` (o usarla davvero nei test, che oggi dipendono da un `sleep` implicito), rendere `reap` periodica invece che per-richiesta, e scrivere i due check separati.

### T5. `SessionUser` fa 2 query per ogni richiesta

`handlers_auth.rs:32-53`: una per la sessione, una per `is_admin`. Un `JOIN sessions/users` con `SELECT s.user_id, u.is_admin, s.expires_at` fa una query sola per ogni richiesta autenticata del server. Con 5 connessioni nel pool e un listing che fa già 3 query, questo è il 25% del carico DB per niente. Inoltre `is_admin` è un `i64` con `unwrap_or(0)`: se la query fallisce, l'utente diventa silenziosamente non-admin invece di dare errore — un fallback che nasconde un guasto.

### T6. `VAULTI_PORT` accetta silenziosamente un valore invalido

`server/src/config.rs:13-16`: `.ok().and_then(|p| p.parse().ok()).unwrap_or(8420)`. `VAULTI_PORT=843O` (lettera O) o `VAULTI_PORT=99999` → porta 8420, in silenzio. Su un gateway in systemd questo significa "il servizio sembra su, ma non dove credi". Un `expect` con messaggio chiaro in fase di avvio è più onesto di un default silenzioso, esattamente come `deploy/README.md` promette.

### T7. `Path`/`String` e `Arc<String>`

`AppState.data_dir: Arc<String>` e `tests/common/mod.rs:21` (`to_string_lossy().to_string()`) sono la conseguenza di B2. `PathBuf` (dentro `Arc`) rende impossibile la concatenazione sbagliata e toglie il `lossy` dai test.

---

## 5. Dimensioni e decomposizione

| file | righe | giudizio |
|---|---|---|
| `server/tests/integration.rs` | **1.275** | **oltre la soglia: va diviso** |
| `server/src/handlers_files.rs` | 364 | al limite alto per 7 handler + 4 DTO; dopo B1/B3 scende a ~230 |
| `server/src/handlers_store.rs` | 318 | contiene 4 flussi distinti (sweep, chunk, finish, cancel) + cleanup |
| `web/js/app.js` | 247 | tre responsabilità: stato di navigazione, rendering DOM, upload con resume |
| `desktop/src-tauri/src/lib.rs` | 238 | plugin, setup, tray, updater, 3 comandi, retry loop |

`integration.rs` è il caso da manuale: file unico, helper definiti **in due posti** (`post_folder`, `make_file`, `range_req`, `finish_status`, `tmp_dir`, `blob_path` stanno nel file di test; `start_upload`, `put_chunk`, `logged_in` stanno in `common/mod.rs`), `use common::*` **e** `common::boot()` usati insieme (righe 6 e ~1180), e `#![allow(dead_code)]` in `common/mod.rs:1` che spegne l'unico avviso che ti direbbe quali helper sono morti. Ci sono anche due test quasi identici (`index_and_assets_served` e `static_assets_no_cache` verificano gli stessi tre path per header diversi).

Decomposizione ovvia, per dominio, senza toccare le asserzioni:

```
tests/common/mod.rs      TestApp + builder delle richieste + helper upload/folder (senza allow(dead_code))
tests/auth.rs            register/login/session/ratelimit/reset-password
tests/upload.rs          chunk/finish/cancel/sweep/507/zero-byte
tests/files.rs           list/folders/rename/move/delete/quota
tests/download.rs        full/range/416/content-disposition
tests/static.rs          asset/mime/404/no-cache
tests/security.rs        origin_guard (nuovo, vedi S2) + cross-user
```

Cinque file da ~200-260 righe invece di uno da 1.275, e l'elenco dei test diventa una mappa delle feature.

`app.js`: lo split naturale è `state.js` (posizione corrente + crumbPath derivato), `render.js` (DOM puro da un modello), `upload.js` (chunk + resume + localStorage). Non è urgente a 247 righe, ma **lo diventa appena si aggiunge cestino o ricerca**, ed è a quel punto che costa il triplo.

---

## 6. Dead code e superficie non esercitata

Cose che esistono ma non fanno nulla, o che nessuno chiama:

| cosa | dove | stato |
|---|---|---|
| `spawn_retry_loop` | `desktop/src-tauri/src/lib.rs:223` | loop che al successo fa `break` e non fa niente (S7) |
| `with_clock` | `server/src/auth.rs:31` | mai usata, nemmeno dai test (T4) |
| `fmtDate` | `web/js/format.js:9` | esportata e testata, mai usata dalla UI |
| `if (!ok) throw` | `web/js/app.js:172` | ramo irraggiungibile (S6) |
| `n &&` in `bfolder` | `web/js/app.js:217` | ridondante dopo `if (!n) return` alla riga 214 |
| `input#q` (Cerca) | `web/index.html:41` | `disabled`, nessun handler in JS |
| `button#lnav-files` | `web/index.html:47` | nessun listener: `.active` è hardcoded |
| `DELETE /api/upload/{id}` | `server/src/handlers_store.rs:298` | nessuna chiamata dal client (solo test) |
| `POST /api/admin/users` | `server/src/handlers_auth.rs:172` | nessuna UI: si possono creare utenti solo con curl |
| `tokio-util` | `server/Cargo.toml:16` | dipendenza dichiarata, `tokio_util` mai importato |

La superficie non esercitata non è solo peso: `cancel` e `admin/users` sono endpoint **non testati dal browser** e quindi non provati contro il middleware CSRF reale (`Origin`/`Sec-Fetch-Site` da un browser vero). Il test li chiama senza quegli header, cioè per la via che in produzione è il fallback. Questo è il tipo di gap che rende S2 urgente.

`fmtDate` e `input#q` possono essere lavoro per la Fase 2 della redesign: se è così, va scritto nel piano; se non lo è, vanno tolti. Codice "in attesa di una feature" è debito con l'interesse più alto.

---

## 7. Cosa è fatto bene (per non rifarlo)

Va detto, perché è la parte che regge:

- **Upload preallocato + `INSERT` prima del `rename`** (`handlers_store.rs:229-281`): l'ordine è pensato per non lasciare mai un blob in `storage/` senza riga, con rollback esplicito se il rename fallisce. È la parte più difficile del progetto ed è corretta.
- **`INSERT OR REPLACE` su `upload_chunks` solo dopo la verifica esatta della lunghezza** (`handlers_store.rs:145-166`): un chunk interrotto non risulta mai ricevuto, il resume è idempotente. Il commento lo spiega e il test `interrupted_chunk_not_marked_received` lo blocca.
- **`INSERT ... SELECT ... WHERE (SELECT COUNT(*) FROM users) = 0`** per il primo utente (`handlers_auth.rs:87-90`): atomico, niente race "due admin". Buono.
- **`client_ip` prende l'ultimo elemento di `X-Forwarded-For`** (`handlers_auth.rs:58-68`), non il primo: corretto e con il commento che spiega perché.
- **Range/download**: `ServeFile` + header `Content-Disposition` con fallback ASCII e `filename*=UTF-8''`, `nosniff`, CSP `default-src 'none'`, `Accept-Ranges`. I test coprono full/206/suffix/416/oltre-fine/nome con caratteri di controllo. Solido.
- **Owner-check prima di toccare il disco** in `chunk` (`handlers_store.rs:108-112`) e path derivato da `stored_name` confermato in `download`.
- **`[hidden] { display: none !important }`** in `base.css:4` con un test CSS che lo blocca: sembra un dettaglio, è la traccia che qualcuno ha già pagato il bug una volta e ha lasciato una rete.
- Le scelte dichiarate (cookie `Secure`+`HttpOnly`+`SameSite=Lax`, password solo via stdin nella CLI, `hash` in `spawn_blocking`) sono giuste.

Nessuna di queste va toccata dai refactor proposti. Le invarianti da preservare sono esattamente: *ordine INSERT→rename*, *verifica esatta prima della riga chunk*, *owner-check prima del filesystem*, *primo utente atomico*.

---

## 8. Piano proposto (ordine, effetto, rischio)

Ogni passo è indipendente, a comportamento invariato, e verificabile con `cargo test` + `node --test web/js/*.test.mjs` (i test esistenti sono la rete: 1.275 righe che coprono i flussi principali — vanno spostate, non riscritte).

| # | Intervento | File toccati | Righe nette | Rischio |
|---|---|---|---|---|
| 1 | Split di `integration.rs` in 6 file + helper consolidati in `common/`, togliere `#![allow(dead_code)]` | `server/tests/**` | 0 (spostamento) | nullo |
| 2 | `AppState` con `PathBuf` + `storage_dir/blob/upload_dir` (B2) | `lib.rs`, 2 handler, `tests/common` | −30 | basso |
| 3 | `AppError` + `Result` in tutti gli handler, `From<sqlx::Error>` con unique→409 (B1) | tutti gli handler, `lib.rs` | −200 | medio (tocca ogni firma) |
| 4 | `discard_upload` unico + sweep fuori dal percorso critico (D1) | `handlers_store.rs` | −40 | basso |
| 5 | `struct Upload` con `expected_len()` + `struct FileRow` (B4) | `handlers_store.rs`, `handlers_files.rs` | −25 | basso |
| 6 | `Patch<T>` + `apply_patch` condiviso (B3) + client che manda `null` | `handlers_files.rs`, `app.js` | −45 | medio (cambia il body PATCH, da aggiornare i test) |
| 7 | `owns` → `folder_of/file_of` tipizzati (S1); cycle-check con CTE (S4) | `handlers_files.rs` | −20 | basso |
| 8 | Modulo `mime` unico (D2); `origin_guard` a origine singola + 4 test (S2) | `auth.rs`, `handlers_static.rs`, `lib.rs`, `tests/security.rs` | −15 | basso |
| 9 | `register`: check "chiusa" prima dell'hash (S5); `RateLimiter` senza `with_clock`, reap periodica (T4); `SessionUser` con JOIN (T5) | `handlers_auth.rs`, `auth.rs` | −40 | basso |
| 10 | Migrazione `0002` per `deleted_at` (B5) | `migrations/`, 2 handler, test | −10 | medio (dati esistenti: nessuno, il campo è sempre NULL) |
| 11 | Desktop: `connect()` unico, comandi `async`, config in `manage`, `--hidden` invece di `!autostart`, decidere `spawn_retry_loop` (D3, S7, S8) | `desktop/src-tauri/src/lib.rs` | −50 | medio (va provato l'installer) |
| 12 | Shell desktop: `shell.css` + `ipc.js` condivisi (D5) | `desktop/ui/**` | −40 | nullo |
| 13 | Frontend: `api()` con body oggetto + JSON automatico, `folderId` derivato da `crumbPath`, retry solo su rete/5xx, via il ramo morto (D4, S6) | `web/js/app.js`, `api.js` | −50 | basso |

Totale stimato: **~550 righe rimosse**, zero concetti nuovi, zero dipendenze nuove (anzi una in meno: `tokio-util`), e ogni passo lascia il progetto verde.

L'ordine non è arbitrario: **1** prima di tutto perché dà una suite più facile da navigare mentre si tocca il resto; **2** prima di **3** perché `AppState` tipizzato rende i `?` di `AppError` naturali; **6** e **10** sono gli unici che cambiano un contratto con il client, e vanno fatti quando la suite è già divisa per dominio.

Cose che **non** ho messo in piano perché sono decisioni di prodotto, non di qualità: cestino (allora B5 si ribalta: `deleted_at` va completata, non rimossa), UI admin, ricerca (`input#q`), quota per utente vera (T2).

---

## 9. Cosa non ho potuto verificare

- **`cargo clippy` / `cargo test` non eseguibili su questa macchina**: il build fallisce prima della compilazione del progetto con `link.exe: extra operand` — il `link` trovato nel PATH non è il linker MSVC (manca il workload "C++ build tools" o è shadowed). Quindi la CI verde è **presunta**, non verificata. Le osservazioni Rust vengono da lettura integrale, non da diagnostica del compilatore.
- **Test JS verificati**: `node --test web/js/*.test.mjs` → 3/3 pass.
- **Nessuna verifica a runtime** del server (healthz, upload reale, browser) e nessuna verifica dell'installer desktop: la review è statica.
- Il conteggio delle righe e degli `unwrap()` è misurato con `grep` su questo albero.

---

## 10. Richiesta di conferma

Non ho modificato nulla. Questo file è la proposta.

Se sei d'accordo, propongo di partire dai passi **1, 2, 4, 5** (tutti a rischio basso, nessun contratto esterno toccato, ~135 righe in meno e la suite divisa per dominio), verificarli, e solo dopo affrontare `AppError` (3), che è il passo grosso e va fatto con calma.

Dimmi anche, prima che parta, come vuoi risolvere i due punti che sono decisioni tue e non mie:

1. **`deleted_at`**: lo elimino (oggi è codice morto) o il cestino è in roadmap e quindi va completato?
2. **`cancel` e `/api/admin/users`**: restano come API "da collegare" o li tolgo finché non hanno una UI?
