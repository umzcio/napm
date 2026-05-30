# napm

A peer-to-peer package manager for your command-line dev tools. It tracks the CLIs you have installed across npm, Homebrew, and pip, shows you what is out of date and whether each update is safe, lets you search the registries and install from the results, and keeps a rollback-able history. The interface is a loving homage to late-90s file-sharing clients.

## Status

Prototype stage. `prototype/napm-prototype.html` is a fully interactive mock with seeded data and no backend. It defines the intended product. The next step is to build the real version against it.

## Repo layout

- `CLAUDE.md` is the build brief. Hand this to Claude Code.
- `prototype/napm-prototype.html` is the interactive UX and visual reference. Open it in any browser.
- `reference/scanner.js` is a working Node CLI that already detects installed and outdated packages across npm, brew, and pip. Lift its logic for the backend agent. Run it with `node reference/scanner.js --demo`.

## Getting started with Claude Code

Open this folder in Claude Code and tell it to read `CLAUDE.md`, then start on step 1 of the build order. The brief documents the architecture, data model, per-feature implementation notes, and the real-world limits of each registry.

## Name

The product is `napm`. It is an npm pun first and a nod to the file-sharing era second. The era styling is homage; the original brand name and logo are deliberately not used.
