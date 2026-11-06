# Vaulti — client Windows (fase B): guscio sottile Tauri

Data: 2026-11-06 · Stato: approvata in revisione (condizionale, applicata v.1.1) · Versione spec: 1.1

## 1. Obiettivo

App desktop Windows per l'uso quotidiano del vault esistente: finestra nativa che
mostra la dashboard del server, trascinamento dei file per caricare, download
salvato in automatico nella cartella Download (§7), chiusura nella tray.
L'app non implementa nulla di proprio: visualizza la stessa dashboard già in
uso nel browser.

Razionale del design scelto (**B, guscio sottile**): la finestra è una webview
che naviga direttamente `https://<host>.ts.net`, l'URL del server. Tutto ciò che
gira nella pagina è servito dal server, quindi **same-origin**: il cookie
`SameSite=Lax` viaggia come nel browser, gli URL relativi di `app.js` puntano al
posto giusto, nessun preflight CORS da gestire. Il design A (frontend Tauri
separato che chiama le API da `http://tauri.localhost`) era stato scartato: è
cross-origin/cross-site, quindi richiederebbe CORS, risposta al preflight
OPTIONS e cookie `SameSite=None` (più debole); inoltre l'allowlist del guard CSRF
su `tauri.localhost` sarebbe imitabile da processi locali. Col guscio sottile
questi problemi non esistono: il guard CSRF e le regole a cookie del server
restano come sono. Le due uniche aggiunte lato server sono piccole e
spiegate in §9: flag `--version` e `Cache-Control: no-cache` su asset statici.

### Fuori ambito

- **Sync automatico** (cartella Windows che si sincronizza da sola,
  OneDrive-like): esplicitamente escluso.
- **Credenziali salvate sul client**: nessuna, né localStorage né keyring.
- **Cestino/purge e anteprime inline**: restano voci separate della fase B
  (`files.deleted_at` è già nello schema), non in questa spec.
- Client per altri OS: solo Windows.

## 2. Architettura

```
[vaulti.exe — Tauri v2]
  ├─ finestra creata da codice ──► naviga https://<host>.ts.net (URL dalla config)
  │     └─ dentro: la dashboard del server (stessa web/, stessi asset embedded)
  ├─ URL assente o server irraggiungibile ──► pagina locale (HTML incluso nell'exe)
  └─ tray + config su disco (parte Rust, poche righe)
```

- Cartella nuova `desktop/` alla radice del repo: `desktop/src-tauri/` con il
  crate Tauri, più `desktop/icon.png` come sorgente delle icone. Il crate server
  cambia solo per le due piccole aggiunte di §9 (`--version` e
  `Cache-Control: no-cache`); tutto il resto del server resta intoccato.
- Il **contenuto remoto non ha capacità IPC**: la capability Tauri è dichiarata
  **senza il campo `remote`**, quindi vale solo per contenuto locale (la pagina
  di configurazione bundlata). Nota sul canale: in Tauri v2 non esiste
  `window.__Tauri__` a meno di `withGlobalTauri`; il canale reale è
  `window.__TAURI_INTERNALS__`. Il test (§10) fa due verifiche: dalla pagina
  locale di configurazione l'`invoke` del comando `save_config` **funziona**
  (controllo positivo, non solo "fallisce"); dalla pagina remota un `invoke`
  dello stesso canale viene **rifiutato dalle regole di accesso**.
- La dashboard in `web/` non si tocca: nessun edit previsto.

## 3. Prima esecuzione e schermata di configurazione

1. L'exe parte e legge `config.json` dalla directory di configurazione
   dell'app, determinata dal `identifier` stabile di `tauri.conf.json`
   (es. `%APPDATA%\net.fenixs.vaulti\config.json`, via
   `app.path().app_config_dir()`).
2. URL assente → mostra una pagina **locale** (HTML incluso nell'exe, nessun
   server web): campo "URL del server" con placeholder `https://nome.ts.net` e
   pulsante "Collega".
3. Validazione lato Rust: **solo `https://`** — ogni URL non-https viene
   rifiutato con messaggio in pagina. Config salvata, si passa al punto 4.
4. **Verifica pre-navigazione dalla parte Rust**: `GET {url}/healthz` (la rotta
   esiste già, `server/src/lib.rs:54`) con timeout di ~3 s. Il controllo serve
   un client HTTP lato Rust (`reqwest` con `rustls`): vive solo nel Rust, mai
   esposto alla pagina.
   - OK → la webview naviga all'URL; il login avviene nella pagina del server
     (stessa dashboard del browser).
   - Fallisce → resta la pagina locale in modalità "Server irraggiungibile" con
     due azioni: **Riprova** (rifà la health check e naviga) e **Cambia
     server** (riapre il modulo di configurazione). Nessuna pagina di errore di
     WebView2: con Tailscale spento l'app rimane comunque in uno stato gestito.
5. Cambio server successivo: da "Cambia server" la config si può riscrivere;
   cambia solo l'URL (le sessioni sono cookie dell'origin del server: non
   esistono credenziali locali da riportare altrove).
6. **Server cade a app aperta** (es. Tailscale si disconnette): la health
   check avviene solo prima della navigazione, quindi un ricaricamento a
   metà sessione potrebbe mostrare la pagina d'errore di WebView2. La via
   di ritorno garantita è la tray, che non dipende dal server: voci
   **Ricarica** (rifà la health check e riparte verso la navigazione o la
   pagina locale) e **Cambia server**. Un ricaricamento diretto (F5/Ctrl+R)
   può mostrare per un attimo l'errore nativo: non è un vicolo cieco, la
   tray sempre presente riporta allo stato gestito.

Config illeggibile o corrotta → stesso percorso del primo avvio (modulo di
configurazione), nessun panic del processo.

## 4. Persistenza della sessione

- Il cookie di sessione del server ha già `Max-Age` di **30 giorni**:
  `COOKIE_MAX_AGE = 30 * 24 * 3600` (`server/src/auth.rs:78`) e la riga del
  cookie `HttpOnly; Secure; SameSite=Lax; Max-Age=…` (`auth.rs:92`; verificato).
- La **scadenza DB della sessione coincide**: al login `expires_at = now() +
  COOKIE_MAX_AGE` (`server/src/handlers_auth.rs:139-142`). Cookie e riga DB
  vivono quindi entrambi 30 giorni: il prossimo login serve solo quando la
  finestra scade (o dopo un reset-password d'emergenza), non a ogni avvio
  dell'app.
- WebView2 ha una directory dati utente persistente (sotto `%LOCALAPPDATA%`
  dell'app): i cookie sopravvivono alla chiusura. Da collaudare al primo uso:
  riavvio dell'app → sessione ancora valida.
- WebView2 mantiene anche una cache HTTP nella stessa directory persistente.
  Gli asset statici non hanno hash nel nome (index.html, app.js, style.css),
  quindi dopo un aggiornamento del server la UI potrebbe restare "vecchia"
  (problema già osservato in browser). Il server li serve con
  `Cache-Control: no-cache` (§9): piccola patch che serve insieme desktop e
  browser.

## 5. Sicurezza

| Rischio | Difesa |
|---|---|
| Contenuto remoto usa il processo desktop (fs, shell, dialog) | Capability **senza campo `remote`**: solo il contenuto locale può invocare. Verifica manuale (§10): dalla pagina remota, `window.__TAURI_INTERNALS__.invoke(...)` in console è rifiutato; dalla pagina locale di config, l'invoke del comando salvataggio config funziona (controllo positivo). |
| Navigazione fuori dall'origin del server | Hook `on_navigation` sulla finestra: consenti **solo l'origin configurato** (confronto scheme+host+porta); ogni altro origin si apre nel **browser di sistema** e la navigazione nella webview viene negata. |
| Nuove finestre (`target="_blank"`, `window.open`) non intercettate da `on_navigation` | Il dashboard non ha link esterni; in ogni caso si verifica a mano dalla console della pagina remota che l'apertura finisca nel browser di sistema e non nella webview (§10). |
| Login/credenziali sul client | Non esistono: la pagina di login è del server, la sessione è cookie HttpOnly dell'origin remoto. |
| URL in chiaro (http) | Rifiutato in configurazione (§3) |

I cookie restano `SameSite=Lax`: essendo la finestra same-origin, la difesa CSRF
del server è invariata (`Sec-Fetch-Site: same-origin`, come nella dashboard
dentro una scheda del browser).

## 6. Finestra, tray, comportamento

- **Finestra creata da codice** (nessuna voce predefinita in `app.windows` di
  `tauri.conf.json`): con config valida si naviga, altrimenti si mostra l'HTML
  locale. Un solo percorso di creazione.
- **X nasconde nella tray** (chiusura finestra → `hide()`, non `close()`); la
  voce **Esci** della tray chiude davvero il processo (`app.exit`).
- Tray con voci **Apri Vaulti** (mostra e attiva la finestra), **Ricarica**
  (health check prima di ogni cosa, poi navigazione o pagina locale) e
  **Cambia server** (torna al modulo di configurazione: via d'uscita
  garantita anche con server spento), oltre a **Esci**; doppio clic
  sull'icona = Apri.
- **`tauri-plugin-single-instance`**: un secondo avvio attiva la finestra già
  aperta ed esce. Serve perché avvio manuale + autostart creerebbero due
  processi e due icone.
- **`tauri-plugin-autostart`** opzionale (disattivo di default). Con autostart
  attivo, all'avvio di Windows Tailscale può non essere ancora su: l'app
  parte allora **nascosta nella tray** e ripete la health check in
  background con backoff (es. ogni 10 s, raddoppiando fino a un tetto)
  finché il server risponde — nessuna finestra in errore in attesa di clic.
- Icone: da `desktop/icon.png`, `cargo tauri icon` produce
  `desktop/src-tauri/icons/` (`icon.ico` incluso). Le icone generate vengono
  committate: il bundle NSIS non può fallire per icona mancante e il CI non
  dipende da passi di generazione.

## 7. Upload e download

- **Upload**: `dragDropEnabled: false` sulla webview, così gli eventi HTML5
  nativi arrivano alla pagina come `File` veri: la pipeline `upload()` esistente
  (chunk 8 MiB, resume, progress) resta identica e funziona anche nel browser.
- **Download**: i file con `Content-Disposition: attachment` terminano con il
  comportamento standard di WebView2: salvataggio **diretto nella cartella
  `Download`** dell'utente Windows, senza dialogo "salva con nome", con la
  barra dei download visibile. Nessuna superficie custom in questa fase: il
  README documenta **dove finisce il file**. Solo se l'esperienza risultasse
  scomoda si valuta l'hook `DownloadStarting` per scegliere/mutare il
  percorso: non previsto ora.

## 8. Versioni, tag, release

- **Tag separati per il desktop**: `desktop-v*` (es. `desktop-v0.1.0`). Un fix
  del server (`v0.1.1`, …) non ricostruisce l'installer, e viceversa.
- Versione sincronizzata col tag: nel job di release, prima di
  `cargo tauri build`, la versione in `tauri.conf.json` viene riscritta dal tag
  (`${GITHUB_REF_NAME#desktop-v}`), così nome e versione interna dell'installer
  coincidono col tag senza edit manuale.
- Asset: NSIS produce `Vaulti_<versione>_x64-setup.exe` → rinominato in
  **`vaulti-setup-x64.exe`** e allegato alla release GitHub `desktop-v*`.
- Disciplina di release: un tag già pubblicato non si muove mai (lezione
  v0.1.0); i fix successivi prendono tag patch nuovi.
- **Visibilità della versione sul server**: flag `vaulti-server --version`
  (stampa la versione ed esce; stesso schema dell'argomento `reset-password` in
  `main.rs`). L'endpoint `GET /api/version` si aggiungerà solo se e quando
  servirà alla dashboard: rimandato.

## 9. Build e CI

- `desktop/` va **escluso dal workspace** del server: `exclude = ["desktop"]`
  nel `Cargo.toml` radice (oggi `members = ["server"]`), così i job Linux non
  scaricano né compilano Tauri.
- Server: i gate attuali restano (fmt + clippy `-D warnings` + test su push e
  tag). Le due sole modifiche al server in questa spec: flag `--version`
  (§8) e `Cache-Control: no-cache` su asset statici (vedi sotto), entrambe
  con test; il guard CSRF resta invariato.
- **Job windows** (su push che tocca `desktop/**` o `.github/workflows/**`, e
  sul tag `desktop-v*`): dentro `desktop/src-tauri` esegue `cargo fmt --check`
  + `cargo clippy -- -D warnings` + `cargo test` — i test unitari del client
  (§10) e la prova che compila. Vale la regola RED-on-CI: niente build
  locale. Nel job: `Swatinem/rust-cache` (la riduzione più economica dei
  tempi) e `desktop/src-tauri/Cargo.lock` **committato** (build
  riproducibili).
- **Job `release-desktop`** (solo tag `desktop-v*`): runner Windows; installare
  il CLI da **binario precompilato** — verificato: le release di
  `tauri-apps/tauri` taggate `tauri-cli-v2.12.4` pubblicano
  `cargo-tauri-x86_64-pc-windows-msvc.zip` (URL diretto risponde 200);
  `taiki-e/install-action` NON supporta `tauri-cli`. Versione **pinzata** nel
  workflow insieme allo **SHA-256 della zip, verificato durante il download**:
  `3e8687fb184bd7f10ebd8642f7715f79f4a605c0694c142a21507e8a24dc0f8b` per
  `tauri-cli-v2.12.4` (se si aggiorna la versione del CLI si aggiorna anche
  il suo hash nel workflow). La zip viene scaricata, verificata e scompattata
  in una directory sul PATH. Poi: riscrittura versione, `cargo tauri
  build` (NSIS), rename, upload dell'asset sulla release del tag.
- Bundle NSIS con `installMode: currentUser` (installazione per utente:
  nessun prompt UAC) e `identifier` stabile in `tauri.conf.json` (es.
  `net.fenixs.vaulti`), che fissa una volta per tutte identità e percorsi
  dell'app.
- Tempi: ogni build desktop su runner Windows dura 10–15 min; con il binario
  precompilato e `rust-cache` si evita la parte peggiore (compilare
  `tauri-cli` da sorgente richiederebbe diversi minuti in più).
- README (desktop): nota **SmartScreen** — l'installer non è firmato, Windows
  avvisa alla prima esecuzione; istruzioni: "Maggiori informazioni → Esegui
  comunque". Documentato anche **dove finiscono i file scaricati**
  (cartella `Download`, §7).

## 10. Test

### Automatizzati (CI)

- Server, unica novità `--version`: integration test che spawna il binario con
  `--version` e verifica che l'output riporti la versione uguale a
  `env!("CARGO_PKG_VERSION")`; più il test della patch statica
  `Cache-Control: no-cache` (§9: risposta senza `Cache-Control` prima, con
  header dopo).
- Client: test unitari in `desktop/src-tauri` per la parte Rust con logica
  reale: validazione URL (https/non-https e casi limite) e carica/salva della
  config. Tutto ciò che dipende da finestra/tray/webview → manuale.

### Manuali (checklist al primo collaudo, utente reale)

1. Prima partenza → compare il modulo URL; un URL http viene rifiutato; un URL
   https valido procede.
2. Server spento (Tailscale disattivo) → pagina locale "irraggiungibile" con
   Riprova / Cambia server; mai la pagina d'errore di WebView2. Server che
   cade **a app aperta**: tray → Ricarica → pagina locale (un ricaricamento
   diretto F5 può mostrare l'errore nativo per un attimo; la via d'uscita
   costante resta la tray).
3. Login (email + password → cookie); riavvio dell'app → **sessione ancora
   valida** (niente re-login).
4. Trascinamento di un file grande (GB) → upload con progress; resume.
5. Download → file salvato nella cartella **`Download`**, barra dei download
   visibile (nessun dialogo "salva con nome"); README documenta dove finisce
   il file.
6. Dalla pagina remota, in console:
   `window.__TAURI_INTERNALS__.invoke(...)` → **rifiutato** dalle regole di
   accesso (nessuna IPC al remoto). Controllo positivo: dalla pagina locale
   di configurazione, l'invoke del comando di salvataggio config (es.
   `save_config`) funziona — altrimenti il rifiuto del remoto non proverebbe
   nulla.
7. Link esterno → si apre nel browser di sistema; navigazione ad altro origin
   negata nella finestra. Dalla console della pagina remota
   `window.open('https://example.com')` (simulazione `target=_blank`) →
   apertura nel browser di sistema, non nella finestra.
8. X nasconde nella tray; "Apri" riporta la finestra; "Esci" termina davvero
   (il processo scompare). Secondo avvio → solo una finestra
   (single-instance).
9. (Facoltativo) autostart attivato → parte al boot.

## 11. Rischi e punti aperti

- **Comodità del download di WebView2**: da valutare in uso reale; hook custom
  solo se necessario (§7, compromesso dichiarato).
- **Installer non firmato**: SmartScreen all'installazione, documentato nel
  README (§9). Nessuna firma code-signing prevista (costo del certificato,
  fuori ambito).
- **Tempi del runner Windows**: 10–15 min a giro; ulteriori riduzioni non sono
  previste oltre ai pin già scelti.
- **Icona di tray su Windows 11**: usa il meccanismo standard di Tauri; se non
  apparisse, si indaga solo quella parte.
- **Directory dati utente di WebView2**: persistente di default; se un giorno
  servisse isolamento tra server diversi, è lì che si tocca (non previsto
  oggi).

## 12. Note di storia del design

- Il design A (frontend Tauri separato + allowlist Origin sul server) era stato
  proposto per primo ed è scartato dopo una revisione tecnica esterna:
  preflight OPTIONS senza handler, cookie `Lax` non inviato cross-site (la
  soluzione `SameSite=None` indebolisce la difesa CSRF), URL relativi puntati
  al posto sbagliato, allowlist imitabile da processi locali. L'eccezione
  Origin che la fase B della spec MVP prevedeva **non serve più** col guscio
  sottile (nessuna modifica al guard).
- Fatto verificato durante la stesura: cookie **e** riga DB della sessione
  condividono gli stessi 30 giorni — nessuna discrepanza da documentare tra
  durata del cookie e durata della sessione.
- Seconda revisione esterna del 2026-11-06 (applicata in v.1.1): test IPC
  sul canale `__TAURI_INTERNALS__` con controllo positivo dal lato locale,
  voci di tray Ricarica/Cambia server per i guasti a app aperta, patch
  server `Cache-Control: no-cache` (cache WebView2 su asset senza hash:
  già vista stantia), autostart nascosto nella tray con retry in backoff,
  `installMode: currentUser` e `identifier` stabile, `Cargo.lock` del
  desktop committato + `rust-cache`, SHA-256 verificato sulla zip del CLI,
  attivazione del job Windows anche da `.github/workflows/**`, test
  manuale per `target=_blank`/`window.open`, download standard (salva in
  `Download` senza dialogo) e client HTTP `reqwest`+`rustls` solo Rust per
  la health check.
