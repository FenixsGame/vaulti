# Vaulti – Piano redesign interfaccia (stile Windows 11 / OneDrive)

Bozza per revisione · 2026-11-07

## 1. Obiettivo e vincoli

Rifare l'interfaccia di Vaulti, il più possibile, con lo stile pulito e chiaro di Windows 11 / OneDrive. I due problemi indicati sono l'organizzazione (navigazione, cartelle, azioni) e l'aspetto grafico (colori, font, spaziature).

Vincoli che restano:

- **Stessa UI per browser e app Windows.** L'app è un guscio che carica la dashboard del server: si lavora su `web/` e basta una nuova release del server, senza reinstallare l'app.
- **Nessun framework e nessuna build** (come oggi), nessun CDN: font di sistema, icone SVG proprie, tutto servito dal server. Punto di decisione a fine Fase 2 (vedi §7).
- **Gratis**, nessuna dipendenza a pagamento.
- **Nessuna IPC per il contenuto remoto** (decisione della spec desktop): niente barra del titolo custom, che richiederebbe chiamate al processo desktop. Si tiene la barra nativa di Windows.
- **Ogni fase è rilasciabile da sola**: finisce con una release del server e un controllo manuale.

## 2. Layout di arrivo

- **Barra superiore:** logo e nome, casella di ricerca al centro, menu utente a destra (account, esci).
- **Pannello sinistro:** I miei file, Recenti, e in fondo la barra dello spazio usato. Condivisi e Cestino compaiono solo quando il server li supporta.
- **Area principale:**
  - breadcrumb cliccabile del percorso;
  - barra comandi: **Nuovo ▾** (Cartella, Carica file), **Carica**, e, con una selezione, Scarica, Rinomina, Sposta, Elimina;
  - interruttore Elenco/Griglia e menu di ordinamento;
  - tabella con colonne Nome, Modificato, Dimensione, icona per tipo di file, intestazione fissa, casella di selezione al passaggio del mouse.
- **Interazioni:** menu tasto destro, doppio clic (apre cartella o scarica file), tastiera (F2 rinomina, Canc elimina, Ctrl+A, frecce, Invio, Backspace sale di un livello).
- **Pannello upload** in basso a destra con progresso per file.
- **Feedback:** stati vuoti, caricamento a scheletro, notifiche (toast) al posto di `alert()`, finestra di conferma prima di eliminare.

Attenzione all'eliminazione: oggi è definitiva (hard delete, nessun cestino, nessun backup). La conferma deve nominare i file e il loro numero.

## 3. Sistema grafico (design token)

CSS variabili in `:root`:

| Token | Valore |
| --- | --- |
| `--bg-app` | `#F3F3F3` |
| `--surface` | `#FFFFFF` |
| `--border` | `#E5E5E5` |
| `--text` / `--text-2` | `#1A1A1A` / `#616161` |
| `--accent` | `#0067C0` (hover `#1975C5`) |
| `--row-hover` | `rgba(0,0,0,.04)` |
| `--row-selected` | `#E5F1FB` |
| `--danger` | `#C42B1C` |
| Raggi | 4 px controlli, 8 px finestre e menu |
| Altezza riga | 40 px |
| Font | `"Segoe UI Variable", "Segoe UI", system-ui, sans-serif`, base 14 px |
| Anello di focus | 2 px accent, sempre visibile da tastiera |
| Transizioni | 120 ms, disattivate con `prefers-reduced-motion` |

Icone: sprite SVG inline (`<symbol>`) disegnato da zero, senza copiare le icone Microsoft. Tipi: cartella, file generico, immagine, video, audio, PDF, archivio, documento; azioni: carica, scarica, elimina, rinomina, sposta, cerca, freccia, altro, chiudi, spunta, info.

## 4. Struttura del codice

`web/` diventa un insieme di moduli ES (funzionano nei browser e in WebView2 senza build):

```
web/index.html
web/css/{tokens,base,components,layout}.css
web/js/{api,state,router,upload,list,menu,dialogs,format}.js
web/icons.svg
```

Server: oggi l'`axum` serve tre file con `include_str!`. Va sostituito con un handler che serve qualunque file di `web/` incorporato (crate `rust-embed`), con il `Content-Type` giusto e `Cache-Control: no-cache`, per evitare una UI vecchia nella cache di WebView2. Test di integrazione: file inesistente 404, tipo MIME corretto, nessuna uscita dalla cartella.

Test della UI senza toolchain locale: la logica pura (formattazione di dimensioni e date, breadcrumb, ordinamento, regole di selezione, validazione dei nomi) sta in moduli senza DOM e si prova in CI con `node --test` (Node è già sui runner GitHub). Il resto si prova con la checklist manuale di ogni fase.

## 5. Fasi

### Fase 0 – Preparazione

- Screenshot dello stato attuale (elenco, upload, login) e lista dei fastidi concreti.
- Branch dedicato e inventario di `web/` e delle API già disponibili (cartelle, spostamento, rinomina, upload in una cartella).
- *Esito:* decisioni dei punti aperti (§8) e conferma del layout.

### Fase 1 – Fondamenta

- Token, reset, componenti (pulsanti, campi, menu, finestre, toast, scheletri) e sprite icone.
- Guscio della pagina (barra superiore, pannello sinistro, area principale) con le funzioni esistenti dentro.
- Handler statico multi-file con `no-cache` (§4) e job CI `node --test`.
- *Fatto quando:* la dashboard ha il nuovo aspetto con tutte le funzioni di prima, e dopo un aggiornamento del server l'app mostra subito la UI nuova.

### Fase 2 – Navigazione

- Pannello sinistro, breadcrumb, apertura cartelle, cronologia avanti/indietro tramite l'hash (`#/f/<id>`), Backspace per salire, stato salvato nell'URL.
- *Fatto quando:* si naviga l'albero senza ricaricare e il pulsante indietro funziona.

### Fase 3 – Elenco file e azioni

- Tabella con selezione (clic, Ctrl, Maiusc, Ctrl+A), barra comandi contestuale, menu tasto destro, rinomina in linea (F2), nuova cartella, elimina con conferma, sposta con selettore di cartella e trascinamento sulle cartelle.
- Ordinamento, vista Griglia, stati vuoto/errore/caricamento.
- Operazioni multiple: all'inizio un ciclo di chiamate esistenti (endpoint bulk solo se serve).
- *Fatto quando:* tutte le azioni dei file si fanno senza lasciare la pagina e la tastiera funziona.

### Fase 4 – Upload e download

- Pannello upload con coda, progresso, velocità e tempo residuo, annulla e riprova; ripresa dopo un ricaricamento (`upload_id` salvato in `localStorage`, ritentativi con backoff).
- Sovrapposizione "Rilascia per caricare in \<cartella>"; upload nella cartella aperta (`folder_id` in `start` se non c'è già).
- Conflitto di nome: finestra con *Mantieni entrambi* (rinomina in `nome (1)`) e *Salta*. *Sostituisci* solo con un endpoint dedicato, da decidere.
- *Fatto quando:* si caricano file grandi interrompendo la rete e riprendendo, senza perdere lo stato.

### Fase 5 – Ricerca, Recenti, spazio

- `GET /api/search?q=` (nome contiene, solo dati dell'utente), vista Recenti (ordinata per data), barra dello spazio usato dai dati già disponibili.
- Paginazione lato server (`limit` e cursore) se le cartelle superano alcune migliaia di righe.

### Fase 6 – Rifinitura

- Tema scuro automatico (`prefers-color-scheme`), contrasto e ruoli ARIA, ordine del focus, guida alle scorciatoie, `prefers-reduced-motion`.

### Fase 7 – Guscio desktop

- Colore di sfondo della finestra uguale all'app (niente lampo bianco), dimensione minima 900×600, titolo "Vaulti".
- Una release `desktop-v*` solo se cambia la configurazione della finestra; altrimenti nessuna reinstallazione.

### Fase 8 – Anteprime (opzionale)

- Rotta separata `?inline=1` solo per tipi in allowlist (immagini, video, audio, PDF, mai HTML o SVG), con `nosniff` e CSP restrittivo. Miniature solo per le immagini all'inizio.

## 6. Test e rilascio

- Logica pura: `node --test` in CI. Server: test di integrazione per ogni endpoint nuovo e per l'handler statico.
- Checklist manuale a fine fase, nel browser e nell'app Windows, con screenshot prima/dopo.
- Ogni fase esce come release del server (`v0.x`) senza spostare tag già pubblicati. Se una fase va male, si torna alla release precedente.

## 7. Rischi

- **Portata ampia:** mitigata dalle fasi rilasciabili da sole. Se serve, ci si ferma dopo la Fase 3 con un'app già molto più usabile.
- **JavaScript che cresce:** moduli piccoli e uno stato centrale. Dopo la Fase 2 si decide se basta o se conviene un file locale di Preact con `htm` (senza build e senza CDN).
- **Nessun test automatico della UI:** compensato da logica pura testata e da checklist manuale.
- **Eliminazione definitiva:** finché non c'è il cestino, conferme chiare e nessun'azione di eliminazione al clic singolo.
- **WebView2 e cache:** si risolve in Fase 1 con `no-cache`.

## 8. Decisioni aperte

1. Vista Griglia e tema scuro: confermati?
2. Anteprime (Fase 8): servono?
3. Conflitto di nome: bastano *Mantieni entrambi* e *Salta*?
4. Vuoi un mockup dello schermo principale prima di scrivere codice (consigliato)?
