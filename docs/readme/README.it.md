<p align="center">
  <img src="../../src-tauri/icons/icon.ico" alt="OxideTerm" width="128" height="128">
</p>

<h1 align="center">⚡ OxideTerm</h1>

<p align="center">
  <strong>Motore Terminale in Rust — Oltre l'SSH</strong>
  <br>
  <em>130.000+ righe di Rust &amp; TypeScript. Zero Electron. Zero dipendenze C nello stack SSH.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.20.1-blue" alt="Versione">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Piattaforma">
  <img src="https://img.shields.io/badge/license-PolyForm%20Noncommercial-blueviolet" alt="Licenza">
  <img src="https://img.shields.io/badge/rust-1.75+-orange" alt="Rust">
  <img src="https://img.shields.io/badge/tauri-2.0-purple" alt="Tauri">
</p>

<p align="center">
  <a href="../../README.md">English</a> | <a href="README.zh-Hans.md">简体中文</a> | <a href="README.zh-Hant.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <a href="README.ko.md">한국어</a> | <a href="README.fr.md">Français</a> | <a href="README.de.md">Deutsch</a> | <a href="README.es.md">Español</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português</a> | <a href="README.vi.md">Tiếng Việt</a>
</p>

---

<p align="center">
  <video src="https://github.com/user-attachments/assets/4ba033aa-94b5-4ed4-980c-5c3f9f21db7e" controls width="100%"></video>
</p>
<p align="center"><em>🤖 OxideSens — «Apri un terminale locale ed esegui echo hello, world!»</em></p>

## Cos'è OxideTerm?

OxideTerm è un'**applicazione terminale multipiattaforma** che unifica shell locali, sessioni SSH remote, gestione file, editing di codice e OxideSens in un singolo binario nativo Rust. **Non** è un wrapper Electron — l'intero backend è scritto in Rust e viene distribuito come eseguibile nativo da 20-35 MB tramite Tauri 2.0.

### Perché un altro terminale?

| Problema | Risposta di OxideTerm |
|---|---|
| I client SSH non supportano la shell locale | Motore ibrido: PTY locale + SSH remoto nella stessa finestra |
| Riconnessione = perdere tutto | **Architettura Node-first**: riconnessione automatica con Grace Period preserva le applicazioni TUI; ripristina forwarding, trasferimenti e stato dell'IDE |
| L'editing remoto richiede VS Code Remote | **Modalità IDE integrata**: editor CodeMirror 6 su SFTP, nessuna installazione server per impostazione predefinita; agente remoto opzionale su Linux |
| Nessun riutilizzo delle connessioni SSH | **Multiplexing SSH**: terminale, SFTP e forwarding condividono un'unica connessione |
| Le librerie SSH dipendono da OpenSSL | **russh 0.54**: SSH puro Rust, backend crittografico `ring`, zero dipendenze C |

---

## Architettura a colpo d'occhio

```
┌─────────────────────────────────────┐
│        Frontend (React 19)          │
│                                     │
│  SessionTreeStore ──► AppStore      │    16 store Zustand
│  IdeStore    LocalTerminalStore     │    20 directory di componenti
│  ReconnectOrchestratorStore         │    11 lingue × 21 namespace
│  PluginStore  AiChatStore  ...      │
│                                     │
│        xterm.js 6 + WebGL           │
└──────────┬──────────────┬───────────┘
           │ Tauri IPC    │ WebSocket (binario)
┌──────────▼──────────────▼───────────┐
│         Backend (Rust)              │
│                                     │
│  NodeRouter ── resolve(nodeId) ──►  │    24 moduli di comandi IPC
│  ├─ SshConnectionRegistry          │    Stato concorrente DashMap
│  ├─ SessionRegistry                │    PTY locale con feature gate
│  ├─ ForwardingManager              │    Vault ChaCha20-Poly1305
│  ├─ SftpSession (livello connessione)│   russh 0.54 (backend ring)
│  └─ LocalTerminalRegistry          │    SSH Agent (AgentSigner)
│                                     │
│  Wire Protocol v1                   │
│  [Type:1][Length:4][Payload:n]       │
└─────────────────────────────────────┘
```

**Comunicazione a doppio piano**: frame binari WebSocket per l'I/O del terminale (zero overhead di serializzazione), IPC Tauri per comandi strutturati ed eventi. Il frontend non accede mai a `sessionId` o `connectionId` — tutto viene indirizzato tramite `nodeId`, risolto lato server dal `NodeRouter`.

---

## Punti di forza tecnici

### 🔩 SSH Puro Rust — russh 0.54

OxideTerm include **russh 0.54** compilato con il backend crittografico `ring`:
- **Zero dipendenze C/OpenSSL** nel percorso SSH — l'intero stack crittografico è in Rust
- Protocollo SSH2 completo: scambio chiavi, canali, sottosistema SFTP, port forwarding
- Suite ChaCha20-Poly1305 e AES-GCM, chiavi Ed25519/RSA/ECDSA

### 🔑 Autenticazione SSH Agent (AgentSigner)

Un `AgentSigner` personalizzato avvolge l'SSH Agent di sistema e soddisfa il trait `Signer` di russh:

```rust
// Risolve il problema RPITIT Send bound in russh 0.54
// clonando &PublicKey in un valore posseduto prima di attraversare l'.await
pub struct AgentSigner { /* ... */ }
impl Signer for AgentSigner { /* challenge-response via IPC dell'Agent */ }
```

- **Piattaforme**: Unix (`SSH_AUTH_SOCK`), Windows (`\\.\pipe\openssh-ssh-agent`)
- **Catene proxy**: ogni salto può usare l'autenticazione Agent in modo indipendente
- **Riconnessione**: `AuthMethod::Agent` viene riprodotto automaticamente alla riconnessione

### 🧭 Architettura Node-First (NodeRouter)

L'**astrazione dei nodi Oxide-Next** elimina un'intera classe di race condition:

```
Frontend: useNodeState(nodeId) → { readiness, sftpReady, error }
Backend:  NodeRouter.resolve(nodeId) → ConnectionEntry → SftpSession
```

- Le operazioni SFTP/IDE del frontend passano solo `nodeId` — nessun `sessionId`, nessun `connectionId`
- Il backend risolve `nodeId → ConnectionEntry` in modo atomico
- La riconnessione SSH cambia `connectionId` — SFTP/IDE **non sono influenzati**
- `NodeEventEmitter` emette eventi tipizzati con contatori di generazione per l'ordinamento

### ⚙️ Terminale Locale — PTY Thread-Safe

Shell locale multipiattaforma tramite `portable-pty 0.8`, protetta dal feature gate `local-terminal`:

- **Thread safety**: `MasterPty` avvolto in `std::sync::Mutex` con `unsafe impl Sync`
- **Thread I/O dedicati**: le letture bloccanti del PTY non toccano mai l'event loop di Tokio
- **Rilevamento della shell**: rileva automaticamente `zsh`, `bash`, `fish`, `pwsh`, Git Bash, WSL2
- **Feature gate**: `cargo build --no-default-features` rimuove il PTY per build mobile

### 🔌 Sistema Plugin Runtime (v1.6.2+)

Caricamento dinamico dei plugin con API congelata e sicurezza rafforzata:

- **API PluginContext**: 8 namespace (terminal, ui, commands, settings, lifecycle, events, storage, system)
- **24 componenti UI Kit**: componenti React precostruiti iniettati nelle sandbox dei plugin
- **Modello di sicurezza**: `Object.freeze` + Proxy ACL, circuit breaker, whitelist IPC
- **Architettura Membrane**: i plugin vengono eseguiti in contesti ESM isolati con un bridge controllato verso l'host

### 🛡️ Pool Connessioni SSH

`SshConnectionRegistry` con conteggio dei riferimenti, basato su DashMap:

- Terminali multipli, SFTP e port forward condividono **un'unica connessione SSH fisica**
- Macchine a stati indipendenti per connessione (connecting → active → idle → link_down → reconnecting)
- Timeout di inattività (30 min), keep-alive (15s), rilevamento guasti tramite heartbeat
- Heartbeat locale WsBridge: intervallo 30s, timeout 5 min (tollera App Nap)
- La disconnessione per inattività emette `connection_status_changed` per notificare il frontend
- Propagazione a cascata: host di salto in errore → tutti i nodi a valle marcati come `link_down`
- **Rilevamento intelligente**: `visibilitychange` + evento `online` → sondaggio SSH proattivo (~2s vs 15-30s passivo)
- **Grace Period**: finestra di 30s per recuperare la connessione esistente prima della riconnessione distruttiva (preserva applicazioni TUI come yazi/vim)

### 🔀 Port Forwarding — I/O Lock-Free

Forwarding locale (-L), remoto (-R) e SOCKS5 dinamico (-D) completo:

- **Architettura a scambio di messaggi**: il canale SSH è proprietà di un singolo task `ssh_io`, nessun `Arc<Mutex<Channel>>`
- **Segnalazione di morte**: i task di forwarding segnalano attivamente il motivo di uscita alla disconnessione SSH
- **Auto-ripristino**: i forwarding `Suspended` riprendono alla riconnessione
- **Timeout di inattività**: `FORWARD_IDLE_TIMEOUT` (300s) previene connessioni zombie

### 🤖 OxideSens

IA a doppia modalità con design orientato alla privacy:

- **Pannello inline** (`⌘I`): comandi rapidi, iniettati tramite bracketed paste
- **Chat laterale**: conversazione persistente con cronologia
- **Acquisizione del contesto**: Terminal Registry raccoglie il buffer dal pannello attivo o da tutti i pannelli divisi
- **Contesto multi-sorgente**: iniezione automatica di file IDE, percorsi SFTP e stato Git nelle conversazioni IA
- **Utilizzo strumenti**: oltre 40 strumenti integrati (operazioni su file, gestione processi, rete, interazione TUI) che l'IA può invocare autonomamente
- **Supporto MCP**: connessione a server esterni [Model Context Protocol](https://modelcontextprotocol.io) (stdio e SSE) per estendere l'IA con strumenti di terze parti — gestiti dalle Impostazioni
- **Compatibile**: OpenAI, Ollama, DeepSeek, OneAPI, qualsiasi endpoint `/v1/chat/completions`
- **Sicuro**: chiavi API nel portachiavi del sistema operativo (macOS Keychain / Windows Credential Manager); su macOS, le letture sono protette da **Touch ID** tramite `LAContext` — senza entitlement o firma del codice richiesta

### � Base di Conoscenza RAG per le Operazioni (v0.20)

Generazione aumentata dal recupero, locale prioritario, per documentazione operativa:

- **Collezioni di documenti**: importa runbook, SOP e guide di deployment in Markdown/TXT in collezioni con ambito globale o per connessione
- **Ricerca ibrida**: indice BM25 per parole chiave + similarità coseno vettoriale, fusi tramite Reciprocal Rank Fusion (RRF)
- **Chunking consapevole del Markdown**: suddivisione per gerarchia di titoli, preservando i percorsi di sezione (es. "Deployment > Docker > Risoluzione problemi")
- **Supporto CJK**: tokenizzatore bigramma per cinese/giapponese/coreano + tokenizzazione per spazi per script latini
- **Integrazione IA**: lo strumento `search_docs` recupera automaticamente il contesto documentale rilevante durante le conversazioni IA — nessun trigger manuale necessario
- **Modifica esterna**: apri documenti nell'editor di sistema, sincronizzazione automatica al ritorno del focus della finestra con blocco ottimistico della versione
- **Reindicizzazione con progresso**: ricostruzione completa BM25 con barra di progresso in tempo reale e supporto alla cancellazione
- **Pipeline di embedding**: il frontend genera vettori tramite il provider IA, memorizzati nel backend per il recupero ibrido
- **Archiviazione**: database embedded redb, 9 tabelle, serializzazione MessagePack con compressione automatica per chunk di grandi dimensioni

### �💻 Modalità IDE — Editing Remoto

Editor CodeMirror 6 su SFTP — nessuna installazione lato server richiesta per impostazione predefinita; Linux supporta un agente remoto leggero opzionale per funzionalità avanzate:

- **Albero dei file**: caricamento lazy con indicatori di stato Git
- **Oltre 30 modalità linguaggio**: 16 nativi di CodeMirror + modalità legacy
- **Risoluzione conflitti**: blocco ottimistico basato su mtime
- **Git basato su eventi**: aggiornamento automatico al salvataggio, creazione, eliminazione, rinomina e pressione di Invio nel terminale
- **State Gating**: I/O bloccato quando `readiness !== 'ready'`, Key-Driven Reset alla riconnessione
- **Agente remoto Linux (opzionale)**: binario Rust di ~1 MB, distribuzione automatica su x86_64/aarch64. Architetture aggiuntive (ARMv7, RISC-V64, LoongArch64, s390x, ecc.) disponibili in `agents/extra/` per caricamento manuale

### 🔐 Export Cifrato .oxide

Formato portabile di backup delle connessioni:

- **ChaCha20-Poly1305 AEAD**: crittografia autenticata
- **Argon2id KDF** (256 MB di memoria, 4 iterazioni) — resistente al brute-force via GPU
- **SHA-256**: checksum di integrità
- **Incorporamento opzionale delle chiavi**: chiavi private codificate in base64 nel payload cifrato
- **Analisi preventiva**: analisi dei tipi di autenticazione, rilevamento chiavi mancanti

### 📡 ProxyJump — Multi-Hop con Topologia

- Profondità di catena illimitata: `Client → Salto A → Salto B → … → Destinazione`
- Analisi automatica SSH Config, costruzione del grafo topologico, calcolo del percorso Dijkstra
- I nodi di salto sono riutilizzabili come sessioni indipendenti
- Propagazione dei guasti a cascata con sincronizzazione automatica dello stato a valle

### 📊 Profiler Risorse

Monitoraggio in tempo reale degli host remoti tramite canale shell SSH persistente:

- Legge `/proc/stat`, `/proc/meminfo`, `/proc/loadavg`, `/proc/net/dev`
- Calcolo della CPU% e del throughput di rete basato su delta
- Canale singolo — evita l'esaurimento di MaxSessions
- Degrada automaticamente a solo RTT su host non Linux o dopo errori consecutivi

### 🖼️ Galleria Immagini di Sfondo

Sistema multi-immagine di sfondo con controllo della trasparenza per scheda:

- **Gestione galleria**: carica più immagini, clicca sulle miniature per cambiare, elimina per immagine o cancella tutto
- **Interruttore principale**: attiva/disattiva lo sfondo globalmente senza eliminare le immagini
- **Controllo per scheda**: 13 tipi di scheda configurabili individualmente (terminale, SFTP, IDE, impostazioni, topologia, ecc.)
- **Personalizzazione**: opacità (3–50%), sfocatura (0–20px), modalità di adattamento (cover/contain/fill/tile)
- **Adattato alla piattaforma**: supporto trasparenza su macOS; percorso WSLg escluso su Windows (canvas VNC opaco)
- **Sicurezza**: eliminazione con percorsi canonicalizzati per prevenire directory traversal; propagazione completa degli errori dal backend Rust

### 🏎️ Rendering Adattivo — Frame Rate Dinamico

Uno scheduler di rendering a tre livelli sostituisce il batching RAF fisso, migliorando la reattività durante output intenso e riducendo il carico GPU/batteria durante l'inattività:

| Livello | Trigger | Frequenza effettiva | Beneficio |
|---|---|---|---|
| **Boost** | Dati del frame ≥ 4 KB | 120 Hz+ (RAF / ProMotion nativo) | Elimina il lag di scorrimento su output rapido |
| **Normal** | Digitazione standard / I/O leggero | 60 Hz (RAF) | Interazione di base fluida |
| **Inattivo** | 3s senza I/O, pagina nascosta o finestra senza focus | 1–15 Hz (timer, crescita esponenziale) | Carico GPU quasi nullo, risparmio batteria |

- **Modalità automatica**: le transizioni sono guidate dal volume dei dati, dall'input utente e dalla Page Visibility API — nessuna regolazione manuale necessaria
- **Sicuro in background**: quando la scheda è nascosta, i dati remoti in arrivo continuano a essere scaricati tramite il timer inattivo — RAF non viene mai attivato, prevenendo l'accumulo di buffer pendenti nelle schede in background
- **Impostazioni**: tre modalità (Auto / Sempre 60 Hz / Disattivato) in Impostazioni → Terminale → Renderer
- **Diagnostica in tempo reale**: attiva **Mostra overlay FPS** per visualizzare un badge di livello in tempo reale (`B`=boost · `N`=normal · `I`=inattivo), il frame rate e il contatore di scritture al secondo fluttuante nell'angolo del terminale

### 🎨 Motore Temi Personalizzati

Personalizzazione dei temi in profondità oltre le palette predefinite:

- **Oltre 30 temi integrati**: Oxide, Dracula, Nord, Catppuccin, Spring Rice, Tokyo Night e altri
- **Editor di temi personalizzato**: selettore visivo dei colori + input esadecimale RGB per ogni campo
- **Colori del terminale**: tutti i 22 campi di xterm.js (sfondo, primo piano, cursore, selezione, 16 colori ANSI)
- **Colori dell'interfaccia**: 19 variabili CSS in 5 categorie — Sfondo (5), Testo (3), Bordi (3), Accento (4), Colori di stato semantici (4)
- **Auto-derivazione**: generazione con un clic dei colori UI dalla palette del terminale
- **Anteprima in tempo reale**: mini terminale in tempo reale + anteprima dell'interfaccia durante la modifica
- **Duplica ed estendi**: crea nuovi temi duplicando qualsiasi tema integrato o personalizzato
- **Persistente**: i temi personalizzati sono salvati in localStorage e sopravvivono agli aggiornamenti dell'applicazione

### 🪟 Ottimizzazione Profonda Windows

- **Integrazione nativa ConPTY**: invocazione diretta dell'API Windows Pseudo Console (ConPTY) per supporto perfetto di TrueColor e sequenze di escape ANSI — nessun WinPTY obsoleto.
- **Rilevamento intelligente delle shell**: scanner integrato che rileva automaticamente **PowerShell 7 (pwsh)**, **Git Bash**, **WSL2** e CMD legacy tramite Registro di sistema e PATH.
- **Esperienza nativa**: Rust gestisce direttamente gli eventi della finestra — velocità di risposta nettamente superiore alle applicazioni Electron.

### 📊 Buffer di Scorrimento Backend

- **Persistenza ad alta capacità**: **100.000 righe** predefinite di output del terminale, serializzabili su disco (formato MessagePack).
- **Ricerca ad alte prestazioni**: `spawn_blocking` isola i task di ricerca regex, evitando di bloccare il runtime Tokio.
- **Efficienza di memoria**: design a buffer circolare che elimina automaticamente i dati più vecchi, mantenendo l'uso della memoria sotto controllo.

### ⚛️ Architettura Multi-Store

Il frontend adotta un pattern **Multi-Store** (16 store) per gestire domini di stato drasticamente diversi:

| Store | Ruolo |
|---|---|
| **SessionTreeStore** | Intento dell'utente — struttura ad albero, flusso di connessione, organizzazione delle sessioni |
| **AppStore** | Livello dei fatti — stato reale delle connessioni SSH tramite Map `connections`, sincronizzato da SessionTreeStore |
| **IdeStore** | Modalità IDE — editing remoto di file, stato Git, editor multi-scheda |
| **LocalTerminalStore** | Ciclo di vita del PTY locale, monitoraggio processi shell, I/O indipendente |
| **ReconnectOrchestratorStore** | Pipeline di auto-riconnessione (snapshot → grace-period → ssh-connect → await-terminal → restore) |
| **TransferStore** | Coda e avanzamento dei trasferimenti SFTP |
| **PluginStore** | Stato runtime dei plugin e registro UI |
| **ProfilerStore** | Metriche del profiler risorse |
| **AiChatStore** | Stato della conversazione OxideSens |
| **SettingsStore** | Impostazioni dell'applicazione |
| **BroadcastStore** | Broadcast dell'input — replica i tasti premuti su più pannelli |
| **CommandPaletteStore** | Stato di apertura/chiusura della palette comandi |
| **EventLogStore** | Registro degli eventi del ciclo di vita della connessione e della riconnessione |
| **LauncherStore** | Stato del launcher delle applicazioni della piattaforma |
| **RecordingStore** | Registrazione e riproduzione delle sessioni del terminale |
| **UpdateStore** | Ciclo di vita dell'aggiornamento automatico (check → download → install) |

Nonostante le diverse sorgenti di stato, la logica di rendering è unificata attraverso i componenti `TerminalView` e `IdeView`.

---

## Stack Tecnologico

| Livello | Tecnologia | Dettagli |
|---|---|---|
| **Framework** | Tauri 2.0 | Binario nativo, ~15 MB, senza Electron |
| **Runtime** | Tokio + DashMap 6 | Completamente asincrono con mappe concorrenti lock-free |
| **SSH** | russh 0.54 (`ring`) | Rust puro, zero dipendenze C, SSH Agent |
| **PTY Locale** | portable-pty 0.8 | Feature-gated, ConPTY su Windows |
| **Frontend** | React 19.1 + TypeScript 5.8 | Vite 7, Tailwind CSS 4 |
| **Stato** | Zustand 5 | 16 store specializzati, sincronizzazione basata su eventi |
| **Terminale** | xterm.js 6 + WebGL | Accelerato via GPU, 60fps+ |
| **Editor** | CodeMirror 6 | 16 pacchetti linguaggio + modalità legacy |
| **Crittografia** | ChaCha20-Poly1305 + Argon2id | AEAD + KDF memory-hard |
| **Archiviazione** | redb 2.1 | DB embedded per sessioni, forwarding, trasferimenti |
| **Serializzazione** | MessagePack (rmp-serde) | Persistenza binaria di buffer/stato |
| **i18n** | i18next 25 | 11 lingue × 21 namespace |
| **SFTP** | russh-sftp 2.0 | Protocollo di trasferimento file SSH |
| **WebSocket** | tokio-tungstenite 0.24 | WebSocket asincrono per il piano dati del terminale |
| **Protocollo** | Wire Protocol v1 | Binario `[Type:1][Length:4][Payload:n]` su WebSocket |
| **Plugin** | ESM Runtime | PluginContext congelato + 24 componenti UI Kit |

---

## Matrice Funzionalità

| Categoria | Funzionalità |
|---|---|
| **Terminale** | PTY locale, SSH remoto, pannelli divisi (H/V), registrazione/riproduzione sessioni (asciicast v2), contesto IA tra pannelli, rendering WebGL, galleria immagini di sfondo, 30+ temi + editor temi personalizzato, palette comandi (`⌘K`), modalità zen (`⌘⇧Z`), scorciatoie dimensione font (`⌘+`/`⌘-`) |
| **SSH** | Pool connessioni, multiplexing, ProxyJump (∞ salti), grafo topologico, pipeline auto-riconnessione |
| **Autenticazione** | Password, chiave SSH (RSA/Ed25519/ECDSA), SSH Agent, certificato, Keyboard-Interactive (2FA), Known Hosts |
| **File** | Browser SFTP a doppio pannello, drag-and-drop, anteprima (immagini/video/audio/PDF/codice/hex), coda trasferimenti |
| **IDE** | Albero file, editor CodeMirror, multi-scheda, stato Git, risoluzione conflitti, terminale integrato |
| **Forwarding** | Locale (-L), Remoto (-R), SOCKS5 dinamico (-D), auto-ripristino, segnalazione di morte, I/O lock-free |
| **IA** | Pannello inline + chat laterale, streaming SSE, inserimento codice, oltre 40 strumenti, integrazione server MCP, contesto multi-sorgente, base di conoscenza RAG, OpenAI/Ollama/DeepSeek |
| **Plugin** | Caricamento ESM a runtime, 8 namespace API, 24 UI Kit, sandboxed, circuit breaker |
| **WSL Graphics** ⚠️ | Visualizzatore VNC integrato (Sperimentale): modalità desktop (9 ambienti) + modalità applicazione (GUI singola), rilevamento WSLg, Xtigervnc + noVNC, riconnessione, feature-gated |
| **Sicurezza** | Crittografia .oxide, portachiavi SO, memoria `zeroize`, TOFU per chiavi host |
| **i18n** | EN, 简体中文, 繁體中文, 日本語, FR, DE, ES, IT, 한국어, PT-BR, VI |

---

## Funzionalità in Evidenza

### 🚀 Esperienza Terminale Ibrida
- **Shell locale a latenza zero**: IPC diretto con i processi locali, latenza quasi nulla.
- **SSH remoto ad alte prestazioni**: flusso binario WebSocket, aggirando l'overhead HTTP tradizionale.
- **Ereditarietà completa dell'ambiente**: eredita PATH, HOME e tutte le variabili d'ambiente — esperienza identica al terminale di sistema.

### 🔐 Autenticazione Diversificata
- **Password**: archiviata in modo sicuro nel portachiavi di sistema.
- **Autenticazione a chiave**: RSA / Ed25519 / ECDSA, scansione automatica di `~/.ssh/id_*`.
- **SSH Agent**: agente di sistema tramite `AgentSigner` (macOS/Linux/Windows).
- **Certificato**: Certificati OpenSSH.
- **2FA/MFA**: autenticazione Keyboard-Interactive.
- **Known Hosts**: verifica della chiave host con TOFU e `~/.ssh/known_hosts`.

### 🔍 Ricerca Full-Text
Ricerca del contenuto dei file nell'intero progetto con caching intelligente:
- **Ricerca in tempo reale**: input con debounce di 300ms e risultati istantanei.
- **Cache dei risultati**: cache con TTL di 60 secondi per evitare scansioni ripetute.
- **Raggruppamento risultati**: raggruppati per file con posizionamento per numero di riga.
- **Evidenziazione delle corrispondenze**: termini di ricerca evidenziati nei frammenti di anteprima.
- **Pulizia automatica**: cache invalidata alla modifica dei file.

### 📦 Gestione File Avanzata
- **Protocollo SFTP v3**: gestore file completo a doppio pannello.
- **Drag-and-drop**: operazioni batch su più file e cartelle.
- **Anteprima intelligente**:
  - 🎨 Immagini (JPEG/PNG/GIF/WebP)
  - 🎬 Video (MP4/WebM) con lettore integrato
  - 🎵 Audio (MP3/WAV/OGG/FLAC) con visualizzazione dei metadati
  - 💻 Evidenziazione del codice (oltre 30 linguaggi)
  - 📄 Documenti PDF
  - 🔍 Visualizzatore esadecimale (file binari)
- **Tracciamento avanzamento**: velocità in tempo reale, barre di progresso, tempo stimato di completamento.

### 🌍 Internazionalizzazione (i18n)
- **11 Lingue**: English, 简体中文, 繁體中文, 日本語, Français, Deutsch, Español, Italiano, 한국어, Português, Tiếng Việt.
- **Caricamento dinamico**: pacchetti linguistici on-demand tramite i18next.
- **Type-safe**: definizioni TypeScript per tutte le chiavi di traduzione.

<details>
<summary>📸 Tutte le 11 lingue in azione</summary>
<br>
<table>
  <tr>
    <td align="center"><img src="../screenshots/overview/en.png" width="280"><br><b>English</b></td>
    <td align="center"><img src="../screenshots/overview/zhHans.png" width="280"><br><b>简体中文</b></td>
    <td align="center"><img src="../screenshots/overview/zhHant.png" width="280"><br><b>繁體中文</b></td>
  </tr>
  <tr>
    <td align="center"><img src="../screenshots/overview/ja.png" width="280"><br><b>日本語</b></td>
    <td align="center"><img src="../screenshots/overview/ko.png" width="280"><br><b>한국어</b></td>
    <td align="center"><img src="../screenshots/overview/fr.png" width="280"><br><b>Français</b></td>
  </tr>
  <tr>
    <td align="center"><img src="../screenshots/overview/de.png" width="280"><br><b>Deutsch</b></td>
    <td align="center"><img src="../screenshots/overview/es.png" width="280"><br><b>Español</b></td>
    <td align="center"><img src="../screenshots/overview/it.png" width="280"><br><b>Italiano</b></td>
  </tr>
  <tr>
    <td align="center"><img src="../screenshots/overview/pt-BR.png" width="280"><br><b>Português</b></td>
    <td align="center"><img src="../screenshots/overview/vi.png" width="280"><br><b>Tiếng Việt</b></td>
    <td></td>
  </tr>
</table>
</details>

### 🌐 Ottimizzazione Rete
- **Architettura a doppio piano**: piano dati (WebSocket diretto) e piano di controllo (IPC Tauri) separati.
- **Protocollo binario personalizzato**: `[Type:1][Length:4][Payload:n]`, zero overhead di serializzazione JSON.
- **Controllo della contropressione**: previene l'overflow di memoria durante il traffico a raffica.
- **Auto-riconnessione**: retry con backoff esponenziale, fino a 5 tentativi.

### 🖥️ WSL Graphics (⚠️ Sperimentale)
- **Modalità desktop**: desktop GUI Linux completi all'interno di una scheda terminale — 9 ambienti desktop (Xfce / GNOME / KDE Plasma / MATE / LXDE / Cinnamon / Openbox / Fluxbox / IceWM), rilevati automaticamente.
- **Modalità applicazione**: avvia una singola applicazione GUI (es. `gedit`, `firefox`) senza un desktop completo — Xtigervnc leggero + Openbox WM opzionale, pulizia automatica alla chiusura dell'app.
- **Rilevamento WSLg**: rilevamento automatico della disponibilità WSLg (socket Wayland / X11) per distribuzione, mostrato come badge nell'interfaccia.
- **Xtigervnc + noVNC**: server X autonomo renderizzato tramite `<canvas>` integrato, con `scaleViewport` e `resizeSession`.
- **Sicurezza**: iniezione di array `argv` (nessun parsing della shell), `env_clear()` + whitelist minima, `validate_argv()` con 6 regole di difesa, limiti di concorrenza (4 sessioni app/distribuzione, 8 globali).
- **Riconnessione**: ristabilimento del bridge WebSocket senza terminare la sessione VNC.
- **Feature-gated**: feature Cargo `wsl-graphics`, comandi stub su piattaforme non Windows.

---

## Avvio Rapido

### Prerequisiti

- **Rust** 1.75+
- **Node.js** 18+ (pnpm raccomandato)
- **Strumenti di piattaforma**:
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio C++ Build Tools
  - Linux: `build-essential`, `libwebkit2gtk-4.1-dev`, `libssl-dev`

### Sviluppo

```bash
git clone https://github.com/AnalyseDeCircuit/OxideTerm.git
cd OxideTerm && pnpm install

# Applicazione completa (frontend + backend Rust + PTY locale)
pnpm tauri dev

# Solo frontend (hot reload sulla porta 1420)
pnpm dev

# Build di produzione
pnpm tauri build

# Kernel leggero — rimuove PTY locale per mobile
cd src-tauri && cargo build --no-default-features --release
```

---

## Struttura del Progetto

```
OxideTerm/
├── src/                            # Frontend — 83K righe TypeScript
│   ├── components/                 # 20 directory
│   │   ├── terminal/               #   Viste terminale, pannelli divisi, ricerca
│   │   ├── sftp/                   #   Browser file a doppio pannello
│   │   ├── ide/                    #   Editor, albero file, dialoghi Git
│   │   ├── ai/                     #   Chat inline + laterale
│   │   ├── graphics/               #   WSL Graphics (desktop VNC + visualizzatore app)
│   │   ├── plugin/                 #   Gestore plugin e UI runtime
│   │   ├── forwards/               #   Gestione port forwarding
│   │   ├── connections/            #   CRUD connessioni e importazione
│   │   ├── topology/               #   Grafo topologia di rete
│   │   ├── layout/                 #   Barra laterale, intestazione, pannelli divisi
│   │   └── ...                     #   Sessioni, impostazioni, modali, ecc.
│   ├── store/                      # 16 store Zustand
│   ├── lib/                        # Livello API, provider IA, runtime plugin
│   ├── hooks/                      # Hook React (eventi, tastiera, toast)
│   ├── types/                      # Definizioni dei tipi TypeScript
│   └── locales/                    # 11 lingue × 21 namespace
│
├── src-tauri/                      # Backend — 51K righe Rust
│   └── src/
│       ├── router/                 #   NodeRouter (nodeId → risorsa)
│       ├── ssh/                    #   Client SSH (12 moduli incl. Agent)
│       ├── local/                  #   PTY locale (feature-gated)
│       ├── graphics/               #   WSL Graphics (feature-gated)
│       ├── bridge/                 #   Bridge WebSocket e Wire Protocol v1
│       ├── session/                #   Gestione sessioni (16 moduli)
│       ├── forwarding/             #   Port forwarding (6 moduli)
│       ├── sftp/                   #   Implementazione SFTP
│       ├── config/                 #   Vault, portachiavi, configurazione SSH
│       ├── oxide_file/             #   Crittografia .oxide (ChaCha20)
│       ├── commands/               #   24 moduli comandi IPC Tauri
│       └── state/                  #   Tipi di stato globale
│
└── docs/                           # 27+ documenti di architettura e funzionalità
```

---

## Roadmap

### 🚧 In Corso (v0.21)

- [x] Base di conoscenza RAG — collezioni di documenti locali con ricerca ibrida BM25 + vettoriale, recupero integrato con IA
- [x] Client MCP (Model Context Protocol) — connettere OxideSens a server di strumenti esterni
- [ ] Ricerca sessioni e cambio rapido

### 📋 Pianificato

- [ ] Forwarding dell'SSH Agent

---

## Sicurezza

| Aspetto | Implementazione |
|---|---|
| **Password** | Portachiavi del SO (macOS Keychain / Windows Credential Manager / Linux libsecret) |
| **Chiavi API IA** | Portachiavi del SO sotto il servizio `com.oxideterm.ai`; su macOS, la lettura delle chiavi richiede **Touch ID** (gate biometrico tramite `LocalAuthentication.framework`, senza entitlement di protezione dati necessari) — le chiavi vengono memorizzate nella cache dopo la prima autenticazione, quindi Touch ID viene richiesto una sola volta per sessione |
| **File di configurazione** | `~/.oxideterm/connections.json` — archivia solo gli ID di riferimento al portachiavi |
| **Export** | .oxide: ChaCha20-Poly1305 + Argon2id, incorporamento chiavi opzionale |
| **Memoria** | `zeroize` cancella i dati sensibili; Rust garantisce la sicurezza della memoria |
| **Chiavi host** | TOFU con `~/.ssh/known_hosts` |
| **Plugin** | Object.freeze + Proxy ACL, circuit breaker, whitelist IPC |

---

## Licenza

**PolyForm Noncommercial 1.0.0**

- ✅ Uso personale / senza scopo di lucro: gratuito
- 🚫 Uso commerciale: richiede una licenza
- ⚖️ Clausola di difesa dei brevetti (Clausola Nucleare)

Testo completo: https://polyformproject.org/licenses/noncommercial/1.0.0/

---

## Ringraziamenti

- [russh](https://github.com/warp-tech/russh) — SSH puro Rust
- [portable-pty](https://github.com/wez/wezterm/tree/main/pty) — PTY multipiattaforma
- [Tauri](https://tauri.app/) — Framework per applicazioni native
- [xterm.js](https://xtermjs.org/) — Emulatore di terminale
- [CodeMirror](https://codemirror.net/) — Editor di codice
- [Radix UI](https://www.radix-ui.com/) — Primitive UI accessibili

---

<p align="center">
  <sub>Costruito con Rust e Tauri — 130.000+ righe di codice</sub>
</p>
