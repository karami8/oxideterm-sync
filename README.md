<p align="center">
  <img src="src-tauri/icons/icon.ico" alt="OxideTerm" width="128" height="128">
</p>

<h1 align="center">⚡ OxideTerm</h1>

<p align="center">
  <strong>Rust-Powered Terminal Engine — Beyond SSH</strong>
  <br>
  <em>95,000+ lines of Rust &amp; TypeScript. Zero Electron. Zero C dependencies in the SSH stack.</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.15.3-blue" alt="Version">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-blue" alt="Platform">
  <img src="https://img.shields.io/badge/license-PolyForm%20Noncommercial-blueviolet" alt="License">
  <img src="https://img.shields.io/badge/rust-1.75+-orange" alt="Rust">
  <img src="https://img.shields.io/badge/tauri-2.0-purple" alt="Tauri">
</p>

<p align="center">
  <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.fr.md">Français</a>
</p>

---

## What Is OxideTerm?

OxideTerm is a **cross-platform terminal application** that unifies local shells, remote SSH sessions, file management, code editing, and AI assistance into a single Rust-native binary. It is **not** an Electron wrapper — the entire backend is written in Rust, shipping as a 20-35 MB native executable via Tauri 2.0.

### Why Another Terminal?

| Pain Point | OxideTerm's Answer |
|---|---|
| SSH clients that can't do local shells | Hybrid engine: local PTY + remote SSH in one window |
| Reconnect = lose everything | **Node-first architecture**: auto-reconnect with Grace Period preserves TUI apps; restores forwards, transfers, IDE state |
| Remote file editing needs VS Code Remote | **Built-in IDE mode**: CodeMirror 6 editor over SFTP, zero server install by default; optional remote agent on Linux |
| No SSH connection reuse | **SSH multiplexing**: terminal, SFTP, forwards share one connection |
| SSH libraries depend on OpenSSL | **russh 0.54**: pure Rust SSH, `ring` crypto backend, no C deps |

---

## Architecture at a Glance

```
┌─────────────────────────────────────┐
│        Frontend (React 19)          │
│                                     │
│  SessionTreeStore ──► AppStore      │    10 Zustand stores
│  IdeStore    LocalTerminalStore     │    17 component directories
│  ReconnectOrchestratorStore         │    11 languages × 18 namespaces
│  PluginStore  AiChatStore  ...      │
│                                     │
│        xterm.js 6 + WebGL           │
└──────────┬──────────────┬───────────┘
           │ Tauri IPC    │ WebSocket (binary)
┌──────────▼──────────────▼───────────┐
│         Backend (Rust)              │
│                                     │
│  NodeRouter ── resolve(nodeId) ──►  │    22 IPC command modules
│  ├─ SshConnectionRegistry          │    DashMap concurrent state
│  ├─ SessionRegistry                │    Feature-gated local PTY
│  ├─ ForwardingManager              │    ChaCha20-Poly1305 vault
│  ├─ SftpSession (connection-level) │    russh 0.54 (ring backend)
│  └─ LocalTerminalRegistry          │    SSH Agent (AgentSigner)
│                                     │
│  Wire Protocol v1                   │
│  [Type:1][Length:4][Payload:n]       │
└─────────────────────────────────────┘
```

**Dual-plane communication**: WebSocket binary frames for terminal I/O (zero serialization overhead), Tauri IPC for structured commands and events. The frontend never touches `sessionId` or `connectionId` — everything is addressed by `nodeId`, resolved server-side by the `NodeRouter`.

---

## Technical Highlights

### 🔩 Pure Rust SSH — russh 0.54

OxideTerm ships with **russh 0.54** compiled against the `ring` crypto backend:
- **Zero C/OpenSSL dependencies** in the SSH path — the entire crypto stack is Rust
- Full SSH2 protocol: key exchange, channels, SFTP subsystem, port forwarding
- ChaCha20-Poly1305 and AES-GCM cipher suites, Ed25519/RSA/ECDSA keys

### 🔑 SSH Agent Authentication (AgentSigner)

A custom `AgentSigner` wraps the system SSH Agent and satisfies russh's `Signer` trait:

```rust
// Solves the RPITIT Send bound issue in russh 0.54
// by cloning &PublicKey to an owned value before crossing .await
pub struct AgentSigner { /* ... */ }
impl Signer for AgentSigner { /* challenge-response via Agent IPC */ }
```

- **Platform**: Unix (`SSH_AUTH_SOCK`), Windows (`\\.\pipe\openssh-ssh-agent`)
- **Proxy chains**: each hop can independently use Agent auth
- **Reconnect**: `AuthMethod::Agent` replayed automatically on reconnect

### 🧭 Node-First Architecture (NodeRouter)

The **Oxide-Next Node Abstraction** eliminates an entire class of race conditions:

```
Frontend: useNodeState(nodeId) → { readiness, sftpReady, error }
Backend:  NodeRouter.resolve(nodeId) → ConnectionEntry → SftpSession
```

- Frontend SFTP/IDE operations only pass `nodeId` — no `sessionId`, no `connectionId`
- Backend resolves `nodeId → ConnectionEntry` atomically
- SSH reconnect changes `connectionId` — SFTP/IDE are **unaffected**
- `NodeEventEmitter` pushes typed events with generation counters for ordering

### ⚙️ Local Terminal — Thread-Safe PTY

Cross-platform local shell via `portable-pty 0.8`, feature-gated behind `local-terminal`:

- **Thread safety**: `MasterPty` wrapped in `std::sync::Mutex` with `unsafe impl Sync`
- **Dedicated I/O threads**: blocking PTY reads never touch the Tokio event loop
- **Shell detection**: auto-discovers `zsh`, `bash`, `fish`, `pwsh`, Git Bash, WSL2
- **Feature gate**: `cargo build --no-default-features` strips PTY for mobile builds

### 🔌 Runtime Plugin System (v1.6.2+)

Dynamic plugin loading with a frozen, security-hardened API:

- **PluginContext API**: 8 namespaces (terminal, ui, commands, settings, lifecycle, events, storage, system)
- **24 UI Kit components**: pre-built React components injected into plugin sandboxes
- **Security model**: `Object.freeze` + Proxy ACL, circuit breaker, IPC whitelist
- **Membrane architecture**: plugins run in isolated ESM contexts with controlled bridge to host

### 🛡️ SSH Connection Pool

Reference-counted `SshConnectionRegistry` with DashMap:

- Multiple terminals, SFTP, port forwards share **one physical SSH connection**
- Independent state machines per connection (connecting → active → idle → link_down → reconnecting)
- Idle timeout (30 min), keep-alive (15s), heartbeat failure detection
- WsBridge local heartbeat: 30s interval, 5 min timeout (tolerates App Nap)
- Idle timeout disconnect emits `connection_status_changed` to notify frontend
- Cascade propagation: jump host down → all downstream nodes marked `link_down`
- **Intelligent detection**: `visibilitychange` + `online` event → proactive SSH probe (~2s vs 15-30s passive)
- **Grace Period**: 30s window to recover existing connection before destructive reconnect (preserves TUI apps like yazi/vim)

### 🔀 Port Forwarding — Lock-Free I/O

Full local (-L), remote (-R), and dynamic SOCKS5 (-D) forwarding:

- **Message-passing architecture**: SSH Channel owned by a single `ssh_io` task, no `Arc<Mutex<Channel>>`
- **Death reporting**: forward tasks actively report exit reason on SSH disconnect
- **Auto-restore**: `Suspended` forwards resume on reconnect
- **Idle timeout**: `FORWARD_IDLE_TIMEOUT` (300s) prevents zombie connections

### 🤖 AI Terminal Assistant

Dual-mode AI with privacy-first design:

- **Inline panel** (`⌘I`): quick commands, injected via bracketed paste
- **Sidebar chat**: persistent conversation with history
- **Context capture**: Terminal Registry gathers buffer from active or all split panes
- **Compatible**: OpenAI, Ollama, DeepSeek, OneAPI, any `/v1/chat/completions` endpoint
- **Secure**: API keys in OS keychain (macOS Keychain / Windows Credential Manager); on macOS, reads are gated behind **Touch ID** via `LAContext` — no entitlements or code-signing required

### 💻 IDE Mode — Remote Editing

CodeMirror 6 editor over SFTP — no server-side installation required by default; Linux supports an optional lightweight remote agent for enhanced capabilities:

- **File tree**: lazy-loaded with Git status indicators
- **30+ language modes**: 16 native CodeMirror + legacy modes
- **Conflict resolution**: optimistic mtime locking
- **Event-driven Git**: auto-refresh on save, create, delete, rename, terminal Enter
- **State Gating**: IO blocked when `readiness !== 'ready'`, Key-Driven Reset on reconnect
- **Linux remote agent (optional)**: ~1 MB Rust binary, auto-deployed on x86_64/aarch64. Extra architectures (ARMv7, RISC-V64, LoongArch64, s390x, etc.) available in `agents/extra/` for manual upload

### 🔐 .oxide Encrypted Export

Portable connection backup format:

- **ChaCha20-Poly1305 AEAD** authenticated encryption
- **Argon2id KDF** (256 MB memory, 4 iterations) — GPU brute-force resistant
- **SHA-256** integrity checksum
- **Optional key embedding**: private keys base64-encoded into encrypted payload
- **Pre-flight analysis**: auth type breakdown, missing key detection

### 📡 ProxyJump — Topology-Aware Multi-Hop

- Unlimited chain depth: `Client → Jump A → Jump B → … → Target`
- Auto-parse SSH Config, build topology graph, Dijkstra path calculation
- Jump nodes reusable as independent sessions
- Cascade failure propagation with automatic downstream status sync

### 📊 Resource Profiler

Live monitoring of remote hosts via persistent SSH shell channel:

- Reads `/proc/stat`, `/proc/meminfo`, `/proc/loadavg`, `/proc/net/dev`
- Delta-based CPU% and network throughput calculation
- Single channel — avoids MaxSessions exhaustion
- Auto-degrades to RTT-only on non-Linux or consecutive failures

### 🖼️ Background Image Gallery

Multi-image background system with per-tab transparency control:

- **Gallery management**: upload multiple images, click thumbnails to switch, per-image delete or bulk clear
- **Master toggle**: enable/disable background globally without deleting images
- **Per-tab control**: 13 tab types individually toggleable (terminal, SFTP, IDE, settings, topology, etc.)
- **Customization**: opacity (3–50%), blur (0–20px), fit mode (cover/contain/fill/tile)
- **Platform-aware**: macOS transparency support; Windows WSLg path excluded (opaque VNC canvas)
- **Security**: path-canonicalized delete prevents directory traversal; full error propagation from Rust backend

### 🎨 Custom Theme Engine

Full-depth theme customization beyond preset palettes:

- **30+ built-in themes**: Oxide, Dracula, Nord, Catppuccin, Spring Rice, Tokyo Night, and more
- **Custom theme editor**: visual color picker + hex RGB input for every field
- **Terminal colors**: all 22 xterm.js fields (background, foreground, cursor, selection, 16 ANSI colors)
- **UI chrome colors**: 19 CSS variables across 5 categories — Background (5), Text (3), Borders (3), Accent (4), Semantic status colors (4)
- **Auto-derive**: one-click generation of UI colors from terminal palette
- **Live preview**: real-time mini terminal + UI chrome preview while editing
- **Duplicate & extend**: create new themes by duplicating any built-in or custom theme
- **Persistent**: custom themes saved to localStorage, survive app updates

### 🪟 Deep Windows Optimization

- **Native ConPTY Integration**: directly invoking Windows Pseudo Console (ConPTY) API for perfect TrueColor and ANSI escape sequence support — no outdated WinPTY.
- **Intelligent Shell Detection**: built-in scanner auto-detects **PowerShell 7 (pwsh)**, **Git Bash**, **WSL2**, and legacy CMD via Registry and PATH.
- **Native Experience**: Rust directly handles window events — response speed far exceeds Electron apps.

### 📊 Backend Scroll Buffer

- **High-capacity persistence**: default **100,000 lines** of terminal output, serializable to disk (MessagePack format).
- **High-performance search**: `spawn_blocking` isolates regex search tasks, avoiding blocking Tokio runtime.
- **Memory efficient**: circular buffer design auto-evicts oldest data, keeping memory usage controlled.

### ⚛️ Multi-Store State Architecture

Frontend adopts a **Multi-Store** pattern (10 stores) to handle drastically different state domains:

| Store | Role |
|---|---|
| **SessionTreeStore** | User intent — tree structure, connection flow, session organization |
| **AppStore** | Fact layer — actual SSH connection state via `connections` Map, synced from SessionTreeStore |
| **IdeStore** | IDE mode — remote file editing, Git status, multi-tab editor |
| **LocalTerminalStore** | Local PTY lifecycle, Shell process monitoring, independent I/O |
| **ReconnectOrchestratorStore** | Auto-reconnect pipeline (snapshot → grace-period → ssh-connect → await-terminal → restore) |
| **TransferStore** | SFTP transfer queue and progress |
| **PluginStore** | Plugin runtime state and UI registry |
| **ProfilerStore** | Resource profiler metrics |
| **AiChatStore** | AI chat conversation state |
| **SettingsStore** | Application settings |

Despite different state sources, rendering logic is unified through `TerminalView` and `IdeView` components.

---

## Tech Stack

| Layer | Technology | Details |
|---|---|---|
| **Framework** | Tauri 2.0 | Native binary, ~15 MB, no Electron |
| **Runtime** | Tokio + DashMap 6 | Full async with lock-free concurrent maps |
| **SSH** | russh 0.54 (`ring`) | Pure Rust, zero C deps, SSH Agent |
| **Local PTY** | portable-pty 0.8 | Feature-gated, ConPTY on Windows |
| **Frontend** | React 19.1 + TypeScript 5.8 | Vite 7, Tailwind CSS 4 |
| **State** | Zustand 5 | 10 specialized stores, event-driven sync |
| **Terminal** | xterm.js 6 + WebGL | GPU-accelerated, 60fps+ |
| **Editor** | CodeMirror 6 | 16 language packs + legacy modes |
| **Encryption** | ChaCha20-Poly1305 + Argon2id | AEAD + memory-hard KDF |
| **Storage** | redb 2.1 | Embedded DB for sessions, forwards, transfers |
| **Serialization** | MessagePack (rmp-serde) | Binary buffer/state persistence |
| **i18n** | i18next 25 | 11 languages × 18 namespaces |
| **SFTP** | russh-sftp 2.0 | SSH File Transfer Protocol |
| **WebSocket** | tokio-tungstenite 0.24 | Async WebSocket for terminal data plane |
| **Protocol** | Wire Protocol v1 | Binary `[Type:1][Length:4][Payload:n]` over WebSocket |
| **Plugins** | ESM Runtime | Frozen PluginContext + 24 UI Kit components |

---

## Feature Matrix

| Category | Features |
|---|---|
| **Terminal** | Local PTY, SSH remote, split panes (H/V), session recording/playback (asciicast v2), cross-pane AI context, WebGL rendering, background image gallery, 30+ themes + custom theme editor, command palette (`⌘K`), zen mode (`⌘⇧Z`), font size shortcuts (`⌘+`/`⌘-`) |
| **SSH** | Connection pool, multiplexing, ProxyJump (∞ hops), topology graph, auto-reconnect pipeline |
| **Auth** | Password, SSH Key (RSA/Ed25519/ECDSA), SSH Agent, Certificate, Keyboard-Interactive (2FA), Known Hosts |
| **Files** | Dual-pane SFTP browser, drag-drop, preview (images/video/audio/PDF/code/hex), transfer queue |
| **IDE** | File tree, CodeMirror editor, multi-tab, Git status, conflict resolution, integrated terminal |
| **Forwarding** | Local (-L), Remote (-R), Dynamic SOCKS5 (-D), auto-restore, death reporting, lock-free I/O |
| **AI** | Inline panel + sidebar chat, streaming SSE, code insertion, OpenAI/Ollama/DeepSeek |
| **Plugins** | Runtime ESM loading, 8 API namespaces, 24 UI Kit, sandboxed, circuit breaker |
| **WSL Graphics** ⚠️ | Built-in VNC viewer (Experimental): Desktop mode (9 DEs) + App mode (single GUI app), WSLg detection, Xtigervnc + noVNC, reconnect, feature-gated |
| **Security** | .oxide encryption, OS keychain, `zeroize` memory, host key TOFU |
| **i18n** | EN, 简体中文, 繁體中文, 日本語, FR, DE, ES, IT, 한국어, PT-BR, VI |

---

## Feature Highlights

### 🚀 Hybrid Terminal Experience
- **Zero-latency local Shell**: direct IPC with local processes, near-zero latency.
- **High-performance remote SSH**: WebSocket binary stream, bypassing traditional HTTP overhead.
- **Complete environment inheritance**: inherits PATH, HOME, and all environment variables — matching system terminal experience.

### 🔐 Diverse Authentication
- **Password**: securely stored in system keychain.
- **Key Auth**: RSA / Ed25519 / ECDSA, auto-scans `~/.ssh/id_*`.
- **SSH Agent**: system agent via `AgentSigner` (macOS/Linux/Windows).
- **Certificate**: OpenSSH Certificates.
- **2FA/MFA**: Keyboard-Interactive authentication.
- **Known Hosts**: host key verification with TOFU and `~/.ssh/known_hosts`.

### 🔍 Full-Text Search
Project-wide file content search with intelligent caching:
- **Real-time search**: 300ms debounced input with instant results.
- **Result caching**: 60-second TTL cache to avoid repeated scans.
- **Result grouping**: grouped by file with line number positioning.
- **Highlight matching**: search terms highlighted in preview snippets.
- **Auto-clear**: cache invalidated on file changes.

### 📦 Advanced File Management
- **SFTP v3 Protocol**: full dual-pane file manager.
- **Drag-and-drop**: multi-file and folder batch operations.
- **Intelligent preview**:
  - 🎨 Images (JPEG/PNG/GIF/WebP)
  - 🎬 Videos (MP4/WebM) with built-in player
  - 🎵 Audio (MP3/WAV/OGG/FLAC) with metadata display
  - 💻 Code highlighting (30+ languages)
  - 📄 PDF documents
  - 🔍 Hex viewer (binary files)
- **Progress tracking**: real-time speed, progress bars, ETA.

### 🌍 Internationalization (i18n)
- **11 Languages**: English, 简体中文, 繁體中文, 日本語, Français, Deutsch, Español, Italiano, 한국어, Português, Tiếng Việt.
- **Dynamic loading**: on-demand language packs via i18next.
- **Type-safe**: TypeScript definitions for all translation keys.

### 🌐 Network Optimization
- **Dual-plane architecture**: data plane (WebSocket direct) and control plane (Tauri IPC) separated.
- **Custom binary protocol**: `[Type:1][Length:4][Payload:n]`, no JSON serialization overhead.
- **Backpressure control**: prevents memory overflow during burst traffic.
- **Auto-reconnect**: exponential backoff retry, up to 5 attempts.

### 🖥️ WSL Graphics (⚠️ Experimental)
- **Desktop mode**: full Linux GUI desktops inside a terminal tab — 9 desktop environments (Xfce / GNOME / KDE Plasma / MATE / LXDE / Cinnamon / Openbox / Fluxbox / IceWM), auto-detected.
- **App mode**: launch a single GUI application (e.g., `gedit`, `firefox`) without a full desktop — lightweight Xtigervnc + optional Openbox WM, automatic cleanup on app exit.
- **WSLg detection**: auto-detect WSLg availability (Wayland / X11 sockets) per distro, shown as a badge in the UI.
- **Xtigervnc + noVNC**: standalone X server rendered via in-app `<canvas>`, with `scaleViewport` and `resizeSession`.
- **Security**: `argv` array injection (no shell parsing), `env_clear()` + minimal whitelist, `validate_argv()` 6-rule defense, concurrency limits (4 app sessions/distro, 8 global).
- **Reconnect**: WebSocket bridge re-establish without killing the VNC session.
- **Feature-gated**: `wsl-graphics` Cargo feature, stub commands on non-Windows platforms.

---

## Quick Start

### Prerequisites

- **Rust** 1.75+
- **Node.js** 18+ (pnpm recommended)
- **Platform tools**:
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio C++ Build Tools
  - Linux: `build-essential`, `libwebkit2gtk-4.1-dev`, `libssl-dev`

### Development

```bash
git clone https://github.com/AnalyseDeCircuit/OxideTerm.git
cd OxideTerm && pnpm install

# Full app (frontend + Rust backend + local PTY)
pnpm tauri dev

# Frontend only (hot reload on port 1420)
pnpm dev

# Production build
pnpm tauri build

# Lightweight kernel — strip local PTY for mobile
cd src-tauri && cargo build --no-default-features --release
```

---

## Project Structure

```
OxideTerm/
├── src/                            # Frontend — 56K lines TypeScript
│   ├── components/                 # 17 directories
│   │   ├── terminal/               #   Terminal views, split panes, search
│   │   ├── sftp/                   #   Dual-pane file browser
│   │   ├── ide/                    #   Editor, file tree, Git dialogs
│   │   ├── ai/                     #   Inline + sidebar chat
│   │   ├── graphics/               #   WSL Graphics (VNC desktop + app viewer)
│   │   ├── plugin/                 #   Plugin manager & runtime UI
│   │   ├── forwards/               #   Port forwarding management
│   │   ├── connections/            #   Connection CRUD & import
│   │   ├── topology/               #   Network topology graph
│   │   ├── layout/                 #   Sidebar, header, split panes
│   │   └── ...                     #   sessions, settings, modals, etc.
│   ├── store/                      # 10 Zustand stores
│   ├── lib/                        # API layer, AI providers, plugin runtime
│   ├── hooks/                      # React hooks (events, keyboard, toast)
│   ├── types/                      # TypeScript type definitions
│   └── locales/                    # 11 languages × 18 namespaces
│
├── src-tauri/                      # Backend — 39K lines Rust
│   └── src/
│       ├── router/                 #   NodeRouter (nodeId → resource)
│       ├── ssh/                    #   SSH client (12 modules incl. Agent)
│       ├── local/                  #   Local PTY (feature-gated)
│       ├── graphics/               #   WSL Graphics (feature-gated)
│       ├── bridge/                 #   WebSocket bridge & Wire Protocol v1
│       ├── session/                #   Session management (16 modules)
│       ├── forwarding/             #   Port forwarding (6 modules)
│       ├── sftp/                   #   SFTP implementation
│       ├── config/                 #   Vault, keychain, SSH config
│       ├── oxide_file/             #   .oxide encryption (ChaCha20)
│       ├── commands/               #   22 Tauri IPC command modules
│       └── state/                  #   Global state types
│
└── docs/                           # 28+ architecture & feature docs
```

---

## Roadmap

### ✅ Shipped (v0.14.0)

- [x] Local terminal (PTY) with feature gating
- [x] SSH connection pool & multiplexing
- [x] SSH Agent authentication (AgentSigner)
- [x] Node-first architecture (NodeRouter + events)
- [x] Auto-reconnect orchestrator (8-phase pipeline with Grace Period)
- [x] ProxyJump unlimited bastion chain
- [x] Port forwarding — local / remote / dynamic SOCKS5
- [x] SFTP dual-pane file manager with preview
- [x] IDE mode (CodeMirror 6 + Git status)
- [x] .oxide encrypted export with key embedding
- [x] AI terminal assistant (inline + sidebar)
- [x] Runtime plugin system (PluginContext + UI Kit)
- [x] Terminal split panes with keyboard shortcuts
- [x] Resource profiler (CPU / memory / network)
- [x] i18n — 11 languages × 18 namespaces
- [x] Keyboard-Interactive auth (2FA/MFA)
- [x] Deep history search (30K lines, Rust regex)
- [x] WSL Graphics — desktop mode + app mode VNC viewer (⚠️ Experimental)
- [x] Background image gallery — multi-image upload, per-tab control, master toggle
- [x] Enhanced media preview — audio/video playback in SFTP browser
- [x] Session recording & playback
- [x] Custom theme engine — 30+ built-in themes, visual editor with hex input, 22 terminal + 19 UI color fields
- [x] Command palette (`⌘K`) — fuzzy search across connections, actions, and settings
- [x] Zen mode (`⌘⇧Z`) — distraction-free full-screen terminal, hides sidebar and tab bar
- [x] Terminal font size shortcuts (`⌘+` / `⌘-` / `⌘0`) with live PTY refit

### 🚧 In Progress

- [ ] Session search & quick-switch

### 📋 Planned

- [ ] SSH Agent forwarding

---

## Security

| Concern | Implementation |
|---|---|
| **Passwords** | OS keychain (macOS Keychain / Windows Credential Manager / Linux libsecret) |
| **AI API Keys** | OS keychain under `com.oxideterm.ai` service; on macOS, key reads require **Touch ID** (biometric gate via `LocalAuthentication.framework`, no data-protection entitlements needed) — keys are cached in memory after first auth, so Touch ID is only prompted once per session |
| **Config files** | `~/.oxideterm/connections.json` — stores keychain reference IDs only |
| **Export** | .oxide: ChaCha20-Poly1305 + Argon2id, optional key embedding |
| **Memory** | `zeroize` clears sensitive data; Rust guarantees memory safety |
| **Host keys** | TOFU with `~/.ssh/known_hosts` |
| **Plugins** | Object.freeze + Proxy ACL, circuit breaker, IPC whitelist |

---

## License

**PolyForm Noncommercial 1.0.0**

- ✅ Personal / non-profit use: free
- 🚫 Commercial use: requires a license
- ⚖️ Patent defense clause (Nuclear Clause)

Full text: https://polyformproject.org/licenses/noncommercial/1.0.0/

---

## Acknowledgments

- [russh](https://github.com/warp-tech/russh) — Pure Rust SSH
- [portable-pty](https://github.com/wez/wezterm/tree/main/pty) — Cross-platform PTY
- [Tauri](https://tauri.app/) — Native app framework
- [xterm.js](https://xtermjs.org/) — Terminal emulator
- [CodeMirror](https://codemirror.net/) — Code editor
- [Radix UI](https://www.radix-ui.com/) — Accessible UI primitives

---

<p align="center">
  <sub>Built with Rust and Tauri — 95,000+ lines of code</sub>
</p>
