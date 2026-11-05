# Deploy Vaulti su gateway Linux (Debian/Ubuntu, apt + systemd)

Il binario è statico (musl): nessuna dipendenza runtime. La dashboard e l'API
viaggiano sulla stessa porta; l'accesso avviene tramite Tailscale, non esposta
a internet.

## 1. Prepara il gateway

    sudo apt update
    sudo apt install -y curl tailscale
    sudo useradd -r -s /usr/sbin/nologin vaulti
    sudo mkdir -p /mnt/data/vaulti
    sudo chown vaulti:vaulti /mnt/data/vaulti

## 2. Installa il binario

    curl -L \
      https://github.com/FenixsGame/vaulti/releases/download/v0.1.0/vaulti-server-linux-musl \
      -o /tmp/vaulti-server
    sudo install -m 755 /tmp/vaulti-server /usr/local/bin/vaulti-server

## 3. Installa e avvia il servizio systemd

    curl -L \
      https://raw.githubusercontent.com/FenixsGame/vaulti/main/deploy/vaulti.service \
      -o /tmp/vaulti.service
    sudo install -m 644 /tmp/vaulti.service /etc/systemd/system/vaulti.service
    sudo systemctl daemon-reload
    sudo systemctl enable --now vaulti
    journalctl -u vaulti -f

Verifica: `ss -ltn | grep 8420` → in ascolto su 127.0.0.1.

## 4. Esponi via Tailscale

    sudo tailscale up
    sudo tailscale serve --bg --https=443 http://127.0.0.1:8420

## 5. Primo utente = admin

Apri `https://<host>.ts.net` nel browser e registra SUBITO il primo utente:
diventa admin e la registrazione si chiude per tutti (403 dopo il primo).
Chiunque nella tailnet potrebbe arrivare prima di te.

## Cambio password admin dimenticata (emergenza)

    sudo -u vaulti sh -c \
      'VAULTI_DATA_DIR=/mnt/data/vaulti /usr/local/bin/vaulti-server reset-password tua@email.ts.net' \
      <<< "NUOVAPASSWORD"

Aggiorna l'hash e invalida tutte le sessioni di quell'utente. La nuova password
passa SOLO via stdin, mai come argomento (non finisce nella history dei processi).

## Note

- Cancellazione = hard delete: **non esiste cestino né backup** (scelta MVP).
  Proteggi i dati a monte (es. RAID/snapshot del volume `/mnt/data/vaulti`).
- Variabili opzionali: `VAULTI_PORT` (default 8420), `VAULTI_CHUNK_SIZE`
  (default 8 MiB), `VAULTI_DATA_DIR` (default `/srv/vaulti`, lo unit lo imposta
  su `/mnt/data/vaulti`).
