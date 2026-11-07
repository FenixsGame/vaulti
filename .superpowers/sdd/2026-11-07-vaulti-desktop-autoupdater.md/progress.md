# SDD ledger — plan: docs/superpowers/plans/2026-11-07-vaulti-desktop-autoupdater.md

Setup: esecuzione integrata (executing-plans) scelta dall'utente; stessa ratifica desktop-thin-shell (main diretto, CI-only con fmt/lockfile locali, RED-on-CI).

Task 1: complete — chiavista con CLI pinnata v2.12.4 (hash zip verificato localmente = identico alla CI): private `C:\Users\Fenixs\.tauri\vaulti.key` + password `vaulti.key.password` (FUORI repo, da conservare: senza entrambe niente firme future), public `vaulti.key.pub`. Secrets GitHub caricati: TAURI_SIGNING_PRIVATE_KEY + TAURI_SIGNING_PRIVATE_KEY_PASSWORD (gh secret list confermato 2026-11-07T09:54Z).
Ruling Task 2: il check updater NON blocca l'avvio (piano origine: pre-finestra con block_on) — spostato in spawn 2 s post-setup: il download HTTP non deve ritardare/appendere il via su reti lente; costo se sbagliato: con aggiornamento in coda l'app si chiude da sola mentre l'utente parte (installer riparte; cookie intatto). Costo opposto: rifiuto silenzioso = l'app continua.
Task 2: plugin in builder chain + spawn updater check (2 s) nel setup (commit in watch).
Task 3: `tauri.conf.json` con `bundle.createUpdaterArtifacts: true` + `plugins.updater.endpoints` (latest release) + pubkey committata (commit in watch).
Task 4: `ci.yml` release-desktop: env firma sul build, passo pwsh `latest.json` (versione dal tag, zip preso via `*.zip.sig` BaseName, URL su release asset), softprops files estesi (exe + zip + latest.json) (commit in watch).
