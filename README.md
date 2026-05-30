<p align="center">
  <img src="assets/npstr-logo.svg" alt="npstr" width="150" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/npstr-AI_Package_Manager-3A32FF?style=for-the-badge&labelColor=080a0f" alt="npstr" />
</p>

<p align="center">
  <strong>napm is the AI Package Manager that thinks it is a 1999 file-sharing client</strong><br/>
  Track every command-line dev tool you have across npm, Homebrew, pip, and npx. See what is outdated, whether the update is safe to take, and roll back when it is not.<br/><br/>
  Your CLIs are the files. The registry is the swarm. Updating is a download.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-alpha-3A32FF?style=flat-square" alt="Alpha" />
  <img src="https://img.shields.io/badge/license-MIT-3A32FF?style=flat-square" alt="MIT" />
  <img src="https://img.shields.io/badge/stack-Tauri%20%7C%20Rust%20%7C%20Vanilla%20JS-6A1BFF?style=flat-square" alt="Stack" />
  <img src="https://img.shields.io/badge/sources-npm%20%7C%20brew%20%7C%20pip%20%7C%20npx-1B5CFF?style=flat-square" alt="Sources" />
</p>

---

## Origin

**napm** stands for **n**pstr **A**I **P**ackage **M**anager. It is an npm pun first and a love letter to the late-90s peer-to-peer era second.

Back then a generation hoarded files inside a gray beveled window: a search bar up top, a list of who was sharing what, a transfers tab crawling along at 56 kbps, a throttle slider nobody understood. napm borrows that exact interface and points it at the thing developers actually hoard today, command-line tools.

Your installed CLIs become your Shared Library. A newer release on the registry is just a peer sharing a better copy. Taking an update is a download. The flame marker still means "everybody has this one."

The name and the styling are homage and parody. The original brand and its logo are deliberately not used. napm's mark is its own: a cat with a shipping box for a head, because a package manager should look like the package.

---

## How It Works

```
Scan your system  -->  See what is outdated  -->  Ask if the update is safe  -->  Update or roll back
 npm / brew / pip / npx     the Shared Library          the What's New feed           the Transfers tab
```

napm runs one batch command per ecosystem to learn what you have installed and what the registries consider current, derives whether each tool is current, outdated, or missing, and then lets you act on it. Every version change is logged so the question "claude-code started misbehaving, what changed and when" actually has an answer.

All shell access and version logic live in a native Rust backend. The frontend never shells out. It only asks.

---

## The Four Panes

| Tab | What it does |
|-----|--------------|
| **Shared Library** | Your installed CLIs across npm, Homebrew, pip, and npx, with installed vs latest, a status glyph, and a pin to freeze a version out of Update All |
| **Search the swarm** | Search the registries (not your disk), sorted by weekly downloads as the trust signal, and install straight from a result |
| **What's New** | One card per available update telling you whether to take it: safe, security, or hold, with the changelog and the signals behind the call |
| **Transfers** | Where versions actually change: real streamed install output, an honest exit code, a rollback-able history |

---

## Features

### Shared Library
- One batch scan per ecosystem (npm, brew, pip, npx), merged into a single view
- Status derived honestly: installed equals latest is current, they differ is an update, no installed version is not installed
- **npx as a first-class source**: tools you have run via `npx` show up with their cached version, and a Promote to global button graduates one into a real managed install
- **Pins**: freeze a tool's version so Update All skips it, while it still shows as outdated so you never lose track

### Transfers
- Real install, update, and rollback commands with stdout and stderr streamed live into the active row
- Success or failure shown from the actual exit code, not a fake progress bar
- A persistent history of every install, update, and rollback with a timestamp and a from/to
- Rollback for npm and pip. Homebrew is gated honestly, since it keeps no old bottles and cannot reliably downgrade

### Search the swarm
- Federated across npm and Homebrew by default, with source-filter chips to scope to one registry
- Weekly downloads shown as the popularity and trust signal, with a flame marker on heavily shared packages
- pip is exact-name lookup only, labeled as such, because PyPI removed its search API and napm does not fake what it cannot do

### What's New
- `security`: a real advisory exists. Always recommend taking it
- `safe`: the release is past a settle threshold with no advisories
- A fresh release with no signal yet is labeled "new, little signal yet" rather than a confident verdict
- Changelogs pulled from the source's GitHub releases

### Aesthetic
- Windows-98 beveled chrome and a VT323 wordmark
- A dial-up connect splash on launch
- Era-flavored "shared by" peer handles
- A throttle slider that intentionally does nothing

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| **Shell** | Tauri v2 (Rust), single native window, no Node runtime shipped |
| **Backend** | Native Rust Tauri commands: scan, ops, registry, what's new, store |
| **Frontend** | Vanilla HTML, CSS, and JS. The prototype is the UI, calling `invoke()` |
| **Sources** | npm, Homebrew, pip, npx via `std::process::Command` |
| **Network** | npm registry, formulae.brew.sh, PyPI, GitHub releases and advisories, cached aggressively |
| **Persistence** | SQLite in the platform app-data directory: pins, history, registry caches |
| **Platform** | macOS first |

---

## Architecture

```
            +-------------------------------+
            |   Tauri WebView (frontend)    |
            |   Win98 chrome, vanilla JS    |
            +---------------+---------------+
                            | invoke()
            +---------------+---------------+
            |   Rust backend (Tauri cmds)   |
            |   scan / ops / registry /     |
            |   whatsnew / store            |
            +---------------+---------------+
                 std::process::Command + HTTP
       +--------+--------+--------+--------+
       |  npm   |  brew  |  pip   |  npx   |
       +--------+--------+--------+--------+
                            |
                  +---------+---------+
                  |  SQLite app-data  |
                  | pins / history /  |
                  |   caches          |
                  +-------------------+
```

---

## Quick Start

napm is a desktop app you build and run locally.

**Prerequisites**
- [Rust](https://rustup.rs) (stable) and a [Tauri v2 system setup](https://v2.tauri.app/start/prerequisites/)
- Node 18+ and npm
- macOS (the only supported target today)

**Run it**
```bash
git clone https://github.com/umzcio/napm.git
cd napm

# install the Tauri CLI and dev tooling
npm install

# build the Rust backend and launch the app
npm run tauri dev
```

The first launch compiles the Rust backend, so give it a minute. After that the dial-up splash plays and your real global npm tools fill the Shared Library.

---

## Project Structure

```
napm/
├── frontend/
│   ├── index.html          # the whole UI: Win98 chrome + vanilla JS
│   └── npstr-logo.svg
├── src-tauri/              # native Rust backend
│   ├── src/
│   │   ├── lib.rs          # Tauri commands (scan_installed, ...)
│   │   └── scan/           # one module per ecosystem
│   │       ├── mod.rs      # InstalledTool + aggregation
│   │       └── npm.rs      # npm scan + version merge (unit tested)
│   ├── icons/              # generated from assets/npstr-logo.svg
│   └── tauri.conf.json
├── reference/scanner.js    # the original CLI, kept as a logic reference
├── prototype/              # the canonical UX mock
├── assets/npstr-logo.svg   # brand source
└── docs/                   # specs and plans
```

---

## Design Decisions

**Why a 90s file-sharing skin?** Because the metaphor is exact. A peer-to-peer client is a list of files, who has them, which copies are newer, and a queue of transfers. Swap "files" for "CLI tools" and you have a package manager. The bit is the skin. The substance underneath is a real, fast tool.

**Why Tauri and native Rust?** napm needs privileged shell access to run npm, brew, and pip, which rules out a pure browser app. Tauri gives a tiny single-binary native window, and keeping every shell call and all version logic in Rust means there is exactly one source of truth and the frontend can never shell out.

**Why downloads-per-week as the trust signal?** In a registry the heavily downloaded package is usually the safe grab. Sorting search by popularity turns a vanity metric into a recommendation, and the flame marker just makes the loud signal visible.

**Why be honest about pip and brew?** PyPI has no search API and Homebrew keeps no old bottles. Pretending otherwise would mean fuzzy pip search that returns nothing and a rollback button that always fails. napm surfaces the limit in the UI instead of papering over it.

---

## Roadmap

Built in vertical slices, each one a working app on its own.

- [x] **M1** Tauri shell plus a real npm Shared Library scan
- [ ] **M2** brew, pip, and npx scans
- [ ] **M3** Transfers: streamed installs, history store, rollback for npm and pip, brew gated, npx promote
- [ ] **M4** Search the swarm: npm, the cached brew index, pip exact lookup
- [ ] **M5** What's New: changelogs plus safe and security verdicts
- [ ] **M6** A packaged macOS app

Deferred on purpose: the `hold` issue-velocity verdict, npx usage-frequency intelligence, and cross-platform support.

---

## License

[MIT](LICENSE). Use it, fork it, keep the era flavor.

---

<p align="center">
  <em>Connected at 56.6 kbps. 4,182,007 peers online.</em><br/>
  <sub>throttle slider purely decorative, like it always was</sub>
</p>
