# Vaulti

Vault per password self-hosted: server Rust + client Windows sottile (guscio
Tauri che presenta l'app web del server). Distribuzione su gateway Linux via
Tailscale: vedi `deploy/README.md`.

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
