# Contributing to napm

Thanks for your interest in napm. Issues and pull requests are welcome, whether it is a bug report, a new ecosystem, or a fix to the era flavor.

## Local development

**Prerequisites**
- [Rust](https://rustup.rs) (stable) and the [Tauri v2 system setup](https://v2.tauri.app/start/prerequisites/)
- Node 18+ and npm
- macOS (the only supported target today)

```bash
git clone https://github.com/umzcio/napm.git
cd napm
npm install
npm run tauri dev      # build the Rust backend and launch the app
```

Run the backend tests before opening a pull request:

```bash
cd src-tauri
cargo test --lib
```

## Project conventions

A few rules keep the project coherent. Please follow them.

- **All shell access and version logic live in the Rust backend.** The frontend never shells out. It only calls `invoke()`. If you need a new capability, add a Tauri command, do not run a process from JavaScript.
- **Every element carries real data.** napm wears a late-90s file-sharing skin, but nothing in the interface is faked. No placeholder values, no decorative control that does nothing. If a piece of data is not available (for example pip has no search API), surface the limit in the UI rather than papering over it.
- **No em dashes** in any UI copy or documentation. Use commas, colons, or parentheses.
- **Do not use the original file-sharing brand name** anywhere, and do not reproduce its logo or trade dress. napm's mark is its own original artwork.

## Pull requests

- Keep changes focused. One concern per pull request.
- Match the surrounding code style and the naming already in use.
- Add or update tests for backend logic. Frontend changes are verified by running the app.
- Describe what you changed and how you verified it.

## Reporting a security issue

If you find a security vulnerability, please do not open a public issue. Email the maintainer or open a private security advisory on GitHub so it can be addressed before disclosure.
