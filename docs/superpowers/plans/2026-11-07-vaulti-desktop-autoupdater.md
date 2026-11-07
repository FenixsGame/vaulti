# Piano — guscio desktop: autoupdater (e rilascio desktop-v0.1.1)

Fonte: richiesta utente del 2026-11-07 ("auto updater senza reinstallare"), spec shell §7, fix console già su main (df70717).

## Obiettivo
Il guscio desktop si aggiorna da solo: al via controlla la release `desktop-v*` più recente; se c'è un aggiornamento firmato lo scarica, verifica con la chiave pubblica e lo installa (l'installer NSIS riparte e riapre l'app). Nessuna UI nuova: niente IPC dal remoto, il controllo è Rust-side.

## Task 1: chiave di firma + secrets GitHub
- CLI pinnata `tauri-cli-v2.12.4` (stessa zip+SHA-256 del job di release) scaricata in locale SOLO per `signer generate` (non compila nulla).
- `cargo-tauri signer generate` → chiave privata in `~/.tauri/vaulti.key` (FUORI dal repo, MAI commit; password vuota), chiave pubblica in `~/.tauri/vaulti.key.pub`.
- `gh secret set TAURI_SIGNING_PRIVATE_KEY` (+PASSWORD con valore vuoto) sul repo.
- Attenzione utente: conservare una copia della chiave privata (se persa = simile reinstall manuale, l'atomeri l'aggiornamento rifiuta le firme nuove).

## Task 2: plugin updater nel guscio (Rust)
- `Cargo.toml`: `tauri-plugin-updater = "2"`.
- `lib.rs`: `.plugin(tauri_plugin_updater::Builder::new().build())`; nel `setup` PRIMA della finestra: `block_on` di `updater().check()` → se `Ok(Some(upd))` → `download_and_install`) — su Windows `install` termina il processo e l'installer rilancia; se install fallisce o non c'è nulla, l'app parte normalmente.
- Errori di check/rete: silenziosi (app parte comunque).

## Task 3: configurazione updater
- `tauri.conf.json`: `bundle.createUpdaterArtifacts: true` + `plugins.updater.endpoints` = `https://github.com/FenixsGame/vaulti/releases/latest/download/latest.json` + `plugins.updater.pubkey` = chiave pubblica del Task 1.

## Task 4: job release-desktop produce gli artefatti updater
- env `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` sui passi che buildano/firmano.
- passo pwsh che trova `*.zip.sig` in `bundle/nsis`, ne ricava `$zipName` (`$sig.BaseName`) e scrive `latest.json` (versione dal tag, `pub_date` UTC, `platforms["windows-x86_64"] = { signature, url del download della zip }` in `releases/download/<tag>/<zipName>`).
- softprops `files`: `vaulti-setup-x64.exe` + `*.zip` + `latest.json`.

## Task 5: rilascio desktop-v0.1.1
- Tag `desktop-v0.1.1` su main, watch run tag, collaudo: cmd assente, aggiornamento-on-demand NON disponibile per la 1.1 stessa (check returns same) — la funzione si riflette con la 1.2.
- Il collaudo §10 restante continua su questa build.

## Rischio principale
- firma/chiave: il build di release FALLISCE (visibile e fixiabile) se le secrets non sono giuste; la verifica runtime fallisce SOLO se la chiave pubblica nel conf non corrisponde (visibile al collaudo del tag).
- il download dell'update blocca il via dell'app per pochi secondi SOLO nel caso "aggiornamento disponibile".
