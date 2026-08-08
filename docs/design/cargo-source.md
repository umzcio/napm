# Design: cargo as a scanned ecosystem

Sign-off: PENDING maintainer review (priority call vs M11 needed)

## Status

This is a design, not a commitment. `docs/ROADMAP.md` names M11 (AI tooling: skills + MCP
connectors) as the stated next milestone. cargo is not on that list. This document exists so the
maintainer can rank "add cargo" against M11 with real information instead of a guess, per Plan
022. No source code changes accompany this document.

## Why this matters

napm's premise is one view of every CLI tool a developer has installed. For a Rust developer
(this project's own author writes Rust), `cargo install`ed tools are invisible today, and not
just uncovered, actively swallowed. `src-tauri/src/scan/manual.rs:63` adds `.cargo` to
`managed_roots()` so the toolchain files (`cargo`, `rustc`, `rustup`, etc, all real symlinks to
`rustup` on this machine, see below) do not flood the library as "unmanaged." The test
`excludes_managed_paths_and_known_names` (`manual.rs:290`) asserts exactly this:
`is_managed(".../.cargo/bin/cargo", "cargo", ...)` is `true`. But `scan/mod.rs::scan_all`
(lines 62-77) only runs npm, brew, pip, npx, and manual. No scanner claims `.cargo`, so anything
`cargo install`ed there is excluded from manual by path and claimed by nobody else. It appears in
no source at all, not even as "unmanaged." That is a stronger failure than a gap: a Rust
developer's actual CLI tools (`ripgrep`, `cargo-edit`, `cargo-watch`, whatever they installed)
are structurally invisible to napm by the very code that protects the toolchain files from being
misclassified.

## Evidence and grounding

Everything below marked "real capture" was produced this session, on this machine, via `Bash`
and `Read`. Everything marked as sourced from `rust-lang/cargo` or `rust-lang/crates.io` was
fetched from those repositories' current `master`/`main` branch this session (via `curl` against
`raw.githubusercontent.com` and `gh api`), not recalled from training data, specifically because
the plan requires grounding in real material rather than assumed API shapes. Code file
references are to this worktree at HEAD `bb85e05`.

Files read for the existing patterns this design mirrors: `src-tauri/src/scan/manual.rs`,
`src-tauri/src/scan/mod.rs`, `src-tauri/src/scan/npm.rs`, `src-tauri/src/scan/publisher.rs`,
`src-tauri/src/scan/size.rs`, `src-tauri/src/intel/osv.rs`, `src-tauri/src/store.rs`,
`src-tauri/src/ops.rs`, `src-tauri/src/http.rs`, `src-tauri/src/search/mod.rs`,
`src-tauri/src/search/brew.rs`, `src-tauri/src/search/pip.rs`, `frontend/index.html`,
`docs/ROADMAP.md`.

## 1. Installed-set truth: `.crates2.json` vs `cargo install --list`

**Recommendation:** parse `.crates2.json` (the v2 tracker file) as the primary source. Fall back
to running and parsing `cargo install --list` only when the JSON file is missing, unreadable, or
fails to deserialize, mirroring how `store.rs::read_json` already treats a missing/corrupt local
JSON file as empty rather than an error.

**Real capture, this machine's actual state.** This dev machine has cargo and rustup installed
(`cargo 1.96.0 (30a34c682 2026-05-25)`, `rustup show` reports two toolchains, `stable` active),
but zero packages ever installed via `cargo install`:

```
$ ls -la ~/.cargo/.crates2.json ~/.cargo/.crates.toml
.rw-r--r-- 0 user 9 Jul 19:38 .crates2.json
.rw-r--r-- 0 user 9 Jul 19:38 .crates.toml

$ cargo install --list
(no output, exit code 0)
```

Both tracker files are literally 0 bytes, not even `{}`. That is not a fluke; it is exactly what
cargo's own loader does. `rust-lang/cargo`'s `src/ops/common_for_install_and_uninstall.rs`
(`InstallTracker::load`) reads the file and, on empty contents, substitutes
`CrateListingV2::default()` in memory without ever writing anything back:

```rust
let v2 = if contents.is_empty() {
    CrateListingV2::default()
} else {
    serde_json::from_str(&contents)...
};
```

So an empty/0-byte file is cargo's own "nothing installed yet" state, not a sign of a broken
scan. `scan/cargo.rs` should treat a 0-byte or missing `.crates2.json` as zero rows, not an
error, the same way `npm::parse_npm("", "")` already returns `Vec::new()` (`scan/npm.rs:137`).

Because this machine has no installs, I cannot capture a POPULATED `.crates2.json` from it. I
did not fabricate one. Instead I read cargo's actual serialization code (same file as above,
current on `master`, matching this machine's 1.96.0 cargo closely enough that the on-disk schema
has been stable across it):

```rust
#[derive(Default, Deserialize, Serialize)]
struct CrateListingV2 {
    installs: BTreeMap<PackageId, InstallInfo>,
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct InstallInfo {
    version_req: Option<String>,   // Some only if installed via `--version`
    bins: BTreeSet<String>,        // every binary this package installed
    features: BTreeSet<String>,
    all_features: bool,
    no_default_features: bool,
    profile: String,               // "debug" or "release"
    target: Option<String>,
    rustc: Option<String>,         // `rustc -V` output at install time
    #[serde(flatten)]
    other: BTreeMap<String, serde_json::Value>,
}
```

The map key is not a bare package name. `PackageId`'s `Serialize` impl
(`rust-lang/cargo/src/workspace/package_id.rs`) writes:

```rust
s.collect_str(&format_args!("{} {} ({})", name, version, source_id.as_url()))
```

producing keys like:

- `"ripgrep 14.1.1 (registry+https://github.com/rust-lang/crates.io-index)"` (a normal install)
- `"my-tool 0.1.0 (git+https://github.com/x/my-tool#abcdef0)"` (`--git`)
- `"my-tool 0.1.0 (path+file:///Users/x/my-tool)"` (`--path`)

So a populated file, reconstructed field-for-field from the real struct above (not invented
data), looks like:

```json
{
  "installs": {
    "ripgrep 14.1.1 (registry+https://github.com/rust-lang/crates.io-index)": {
      "version_req": null,
      "bins": ["rg"],
      "features": [],
      "all_features": false,
      "no_default_features": false,
      "profile": "release",
      "target": "aarch64-apple-darwin",
      "rustc": "rustc 1.96.0 (30a34c682 2026-05-25)"
    }
  }
}
```

**The `--list` fallback, also grounded in real source, not guessed.** `rust-lang/cargo`'s
`src/ops/cargo_install.rs` prints installs by iterating `tracker.all_installed_bins()`:

```rust
for (k, v) in tracker.all_installed_bins() {
    drop_println!(gctx, "{}:", k);
    for bin in v { drop_println!(gctx, "    {}", bin); }
}
```

where `k` here uses `PackageId`'s `Display` (not `Serialize`), which omits the source suffix for
crates.io packages and keeps it for anything else:

```
ripgrep v14.1.1:
    rg
cargo-edit v0.12.3:
    cargo-add
    cargo-rm
    cargo-set-version
my-tool v0.1.0 (https://github.com/x/my-tool#abcdef0):
    my-tool
```

On this machine that command produces nothing at all (confirmed above), consistent with zero
installs. `--list` carries strictly less information than the JSON (no `version_req`, `profile`,
`target`), so it earns fallback status only. It is worth noting as a second, independent
confirmation of design question 5: cargo's own printer already treats one `PackageId` (crate) as
the header and its `bins` as an indented, nested list under it, the same "one row, several
binaries" shape recommended below.

**Install-root wrinkle, also real, not hypothetical.** `.crates2.json` is not hardcoded to
`~/.cargo`. `InstallTracker::load(gctx, root: &Filesystem)` opens it relative to whatever `root`
the caller resolved. `rust-lang/cargo`'s own docs
(`doc/man/includes/description-install-root.md`) state the resolution order:

> The installation root is determined, in order of precedence: `--root` option,
> `CARGO_INSTALL_ROOT` environment variable, `install.root` Cargo config value, `CARGO_HOME`
> environment variable, `$HOME/.cargo`.

This machine has neither `CARGO_INSTALL_ROOT` nor a custom `CARGO_HOME` set, so
`~/.cargo/.crates2.json` happens to be correct here, but that is a property of this machine, not
a guarantee. `scan/cargo.rs` should resolve the same chain (at minimum `CARGO_INSTALL_ROOT`, then
`CARGO_HOME`, then `$HOME/.cargo`) rather than hardcoding the path, or it silently misses every
install for anyone who has customized either variable. This same resolved root also matters for
design question 5's exclusion fix.

## 2. Latest resolution: crates.io lookup and where it fits the cache design

**Recommendation:** use the crates.io sparse index (`index.crates.io`) as the primary
latest-version source, not the `/api/v1/crates/<name>` JSON endpoint the plan's evidence draft
named. Real policy research changes this from the plan's starting assumption.

I fetched `rust-lang/crates.io`'s actual Data Access Policy page source
(`svelte/src/routes/data-access/+page.svelte`) rather than relying on memory. It states, for the
sparse index:

> "No rate limits are required to use data from the sparse crate index."

versus, for the JSON API:

> "1. A maximum of 1 request per second, and 2. A `user-agent` header that identifies your
> application. We strongly suggest providing a way for us to contact you (whether through a
> repository, or an e-mail address, or whatever is appropriate)..."

I also fetched the actual middleware that enforces this
(`src/middleware/require_user_agent.rs` and its `no_user_agent_message.txt`), which is the real
403 body a bad User-Agent gets back:

> "Bad: `User-Agent: reqwest/0.9.1` / Better: `User-Agent: my_crawler` / Best:
> `User-Agent: my_crawler (my_crawler.com/info)` or `(help@my_crawler.com)`"

**Does napm's existing UA satisfy this?** `src-tauri/src/http.rs:15` sets
`.user_agent("napm")` on the one shared `ureq` agent every source uses. That is non-empty and not
the CDN sentinel string the middleware special-cases, so it will not be blocked. It sits at
"Better," not "Best": it identifies the app but carries no contact surface, which is the specific
thing crates.io's own policy asks for "to reduce the chance we will need to take action against
your bot." This is a real, if soft, finding: the current UA clears the bar that avoids a 403, but
not the bar crates.io actually asks for. Two follow-ups, flagged but out of scope for a
cargo-only change since they touch every source:

- Bump the shared agent string to something like `"napm (github.com/umzcio/napm)"`. Cheap, and
  benefits npm/brew/pip/OSV too, not just a new cargo source.
- Because the sparse index has no stated rate limit and the JSON API asks for 1 req/sec, a
  library-wide "resolve latest for every installed crate" pass (the cargo equivalent of
  `npm outdated -g`) is cheap and safe against the sparse index. Pointed at the JSON API instead
  for the same fan-out, it would need to be serialized to roughly 1/sec, or the richer
  description/owners metadata deferred lazily on demand (the same lazy-detail pattern
  `intel/osv.rs::fetch_advisory`, lines 159-162, already uses for malicious-package summaries).

**Where it slots into the registry-cache design.** `search/mod.rs::SearchResult` plus
`search/brew.rs`'s `cached_or_fetch` (24h TTL, `search/brew.rs:84-112`) is the existing "fetch
once, reuse" shape. Cargo becomes a third eco arm in `search/mod.rs` alongside npm/brew/pip:
`search/cargo.rs::search_cargo(query)`. crates.io has no free-text search API either (like PyPI,
unlike npm), so this is an exact-name lookup against the sparse index, landing in the same
"labeled exact match" bucket pip already occupies. `frontend/index.html:456` already renders an
`exact` tag keyed off `p.eco==="pip"`; extending that condition to `|| p.eco==="cargo"` is the
honest disclosure move, not a fuzzy-search claim napm cannot back up.

## 3. `--git` / `--path` installs

**Recommendation:** show them, with the installed version known from `.crates2.json`, but with
no latest version and no Update action offered. This is the "unmanaged-flavored" pattern the plan
names: version known, no update path claimed, exactly how `manual.rs` rows already behave
(`latest` set equal to `installed`, no Update, no Roll back, excluded from What's New and the
safe-count).

**Correction to the plan's evidence note, grounded in the real struct.** The plan's evidence
section says this is "recorded in `.crates2.json`'s source field." There is no such field.
`InstallInfo` (quoted in full under design question 1) has no `source` member at all. The source
is encoded entirely in the `PackageId` string used as the map KEY: the parenthesized suffix
(`registry+...`, `git+...#<rev>`, `path+file://...`). `scan/cargo.rs` must parse that key suffix
to classify a row, not look for a field that does not exist on `InstallInfo`.

Recommended classification, keyed off the parsed suffix: `registry+` gets a real latest lookup
per design question 2; `git+` or `path+` gets `latest = installed` and a UI label ("from git" /
"from path") distinct from the normal outdated/current badges, mirroring the distinct "unmanaged"
badge `docs/ROADMAP.md`'s M9 section describes for manual installs. This keeps the same honesty
rule the rest of the app already follows: never imply an update path that does not exist.

## 4. Metadata columns: publisher, size, description

**Publisher.** `docs/ROADMAP.md`'s "Real-metadata pass" states the existing project-wide
convention plainly: "Shared By = real package publisher (author, else repository/homepage GitHub
owner, else domain)... offline," and `scan/publisher.rs` is entirely offline today (parses local
`package.json`/pip `METADATA`, never makes a network call for this column). Recommend the same
for cargo: read the extracted crate's `Cargo.toml` `authors` array from disk.

Real capture proving this file exists and has the right shape, from this machine's cargo
registry SOURCE cache (populated by building napm's own Rust dependencies via `src-tauri`'s
`Cargo.toml`, not by a `cargo install`, but the on-disk format is identical either way since both
paths extract via the same registry-source mechanism):

```
$ cat ~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/adler2-2.0.1/Cargo.toml
# THIS FILE IS AUTOMATICALLY GENERATED BY CARGO
...
[package]
name = "adler2"
version = "2.0.1"
authors = [
    "Jonas Schievink <jonasschievink@gmail.com>",
    "oyvindln <oyvindln@users.noreply.github.com>",
]
description = "A simple clean-room implementation of the Adler-32 checksum"
```

Running the existing `author_name_from_string` / `to_handle` pipeline
(`scan/publisher.rs:51-58, 11-31`) against `authors[0]` here yields `jonas-schievink`, the same
normalized-handle shape npm and pip publishers already get. No new parsing rules are needed, only
a TOML read where npm reads JSON and pip reads a `METADATA` text file.

**Size.** The installed BINARY under the resolved install root's `bin/` (see design question 1),
not the crate's source tree in `registry/src` (which is a general, shared download cache, not
specific to any one install, and can be many times the binary's size). `scan/npm.rs::enrich` and
`manual.rs`'s use of `size::dir_size`/`size::human_size` already draw exactly this distinction
for their own sources. `scan/cargo.rs` should call `size::human_size(size::dir_size(&bin_path))`
per binary named in a row's `bins`, summed when a crate installs more than one (design
question 5).

**Description, and the real tension the plan flagged.** crates.io fetch (rich, current, costs a
network call per crate) versus the offline `Cargo.toml` `description` field (present in the
sample above, real, on disk). Recommend offline-first, matching `docs/ROADMAP.md`'s explicit,
already-shipped policy: "Description = one-line summary... (offline)." The tension is real and
worth stating precisely: unlike npm (`package.json` always ships inside the installed package
folder) and pip (`METADATA` always ships inside the installed dist-info folder), a cargo binary
install does not durably guarantee its extracted source tree stays on disk. `registry/src` is a
general cache the user (or a future napm cache-clearing feature) could prune independently of the
installed binary, since it is not scoped per-install the way npm's and pip's local package
metadata are. When the source folder for the exact installed version is absent, the honest
fallback is an empty description, matching `publisher.rs`'s existing convention throughout
(empty string, never fabricated) rather than a stale or wrong one. A live crates.io fetch is a
defensible enrichment layered on top later, cached the same way brew's catalog is, but v1 should
ship with the offline `Cargo.toml` read alone, matching every other source's v1 scope.

## 5. Multi-binary crates and the exclusion problem

**Recommendation:** one row per crate, not per binary. `InstallInfo.bins: BTreeSet<String>`
(confirmed under design question 1) makes this the natural unit: a crate like `cargo-edit` is one
`PackageId` key mapping to `bins: {"cargo-add", "cargo-rm", "cargo-set-version"}`. cargo's own
`--list` output (confirmed above) independently corroborates this: one header line per crate, N
indented bin lines under it, never the reverse. `InstalledTool` has no `bins: Vec<String>` field
today; the minimal-diff move is `pkg`/`name` = crate name, with the bin list threaded through only
as far as the exclusion mechanism below needs it. A "N binaries" expander/tooltip is a UI nicety
for later, not required for v1.

**The exclusion problem, and the real precedent for how NOT to solve it.**
`scan/mod.rs::scan_all` (lines 62-77) builds `other_names` from
`all.iter().map(|t| t.name.clone())` after every named-ecosystem scan runs and before
`manual::scan_manual` runs, so manual never re-lists a binary another scanner already claimed.
Today, before any cargo scanner exists, the ONLY thing stopping a cargo-installed binary from
leaking into manual as "unmanaged" is the path-prefix exclusion (`.cargo` is a managed root,
`manual.rs:63`), not a name match.

`docs/ROADMAP.md`'s own M9 postmortem is the load-bearing precedent here, and it describes
exactly the bug class the plan warns about for cargo, already shipped and already fixed once, for
a different pair of sources:

> "Found and fixed in the milestone-end review: npm/npx globals and pip user scripts leaking in
> as 'manual' (the name-based exclusion could not catch them because the scan rows carry
> package/distribution names, not binary basenames; fixed by adding the `npm root -g` and
> pip user-script path roots)"

That is the important, real detail: npm/pip's actual fix was not "collect every binary name into
`other_names`," it was "add a path-root exclusion for where those binaries actually live"
(`manual.rs:71-86` shells out to `npm root -g` and `python3 -c "import site;
print(site.getuserbase())"` at scan time to build those roots). The reason: a package's declared
name and its installed binary's basename can differ (scoped npm packages, pip console-script
entry points), so bare-name matching alone is fundamentally unreliable, which is exactly cargo's
`cargo-edit` -> `cargo-add`/`cargo-rm`/`cargo-set-version` situation too.

For cargo, the DEFAULT install root (`$HOME/.cargo/bin`) is already inside the existing `.cargo`
managed root, so the path-based guard covers the common case for free, before any cargo-specific
code exists. Design question 1's finding is exactly where that free protection breaks: a user
with `CARGO_INSTALL_ROOT` (or `install.root`) pointed somewhere else has cargo-installed binaries
outside every existing managed root, with names that cannot be relied on to match the crate name
either. The correct fix, mirroring the real npm/pip precedent instead of inventing a new
mechanism: resolve the same install root cargo would use (once, at `manual.rs::managed_roots`
build time, alongside the existing `npm root -g` / pip user-base calls) and add its `bin/` as a
path root. Name-based `other_names` exclusion (adding every string in every cargo row's `bins`,
not just the crate name, to the existing set) should still be added as a second, cheap,
independent layer, because the two mechanisms fail differently: path-root exclusion breaks if the
root cannot be resolved; name exclusion breaks if a bin name happens to collide with something
unrelated already on PATH. The M9 lesson is that neither alone is enough; this is exactly the
kind of case that shipped broken twice already (`321c391 fix(m9): don't exclude /usr/local/bin`,
`e0d5d32 fix(m9): exclude npm-global and pip-script dirs`, both real commits in this repo's
history) and must not ship broken a third time for cargo.

**The test this must carry:**

- A fixture cargo row for a multi-binary crate (`cargo-edit` with `bins` = `cargo-add`,
  `cargo-rm`, `cargo-set-version`) feeding `scan_all`'s `other_names` construction, asserting all
  three bin names, not just `cargo-edit`, end up excluded from a subsequent `scan_manual` call. In
  practice: a fixture "manual" binary literally named `cargo-add`, sitting outside any managed
  root, must not appear as an unmanaged row once a cargo scan already claims it. This mirrors the
  shape of `manual.rs`'s existing `excludes_managed_paths_and_known_names` test.
- A companion path-root test: with the install-root resolution seam pointed outside `.cargo`, the
  resolved `bin/` dir is still added to `managed_roots()`, so an unrelated, entirely unscanned
  binary dropped in that same custom root is still excluded by path. This is the actual
  load-bearing M9-style fix; the name-list test above is the second, independent layer, not a
  substitute for it.

## 6. Go: recommend no

**Recommendation:** do not add a Go scanner. Record this as an explicit non-goal, not a silent
gap.

This section is general Go tooling knowledge, not a per-machine capture; this machine's cargo
state is the plan's specific grounding requirement and I have not run or captured any Go tooling
this session. `go install` writes binaries into `$GOBIN`/`$GOPATH/bin` but keeps no manifest
anywhere of what was installed, at what version, or from where. Recovering that requires walking
every binary in the bin directory and running `go version -m <binary>` per binary (Go 1.18+
embeds module version info in the binary itself) to recover a version and module path per file.
That is not a batch command with one parse; it is exactly the slow, per-binary, timeout-bounded
introspection pattern `manual.rs::resolve_version` exists to make bearable as a LAST RESORT for
genuinely unmanaged binaries, not something to promote into a fast-path scanner. Standing up a
`scan/go.rs` would mean re-implementing manual's slow path as if it were a fast one, for every
Go-installed tool, worse than letting the existing `manual.rs` PATH sweep keep catching them,
labeled "unmanaged," honestly, today. `~/go/bin` is not in `managed_roots()` today and should
stay that way if Go remains a non-goal, since manual is the only source currently telling the
user anything about their Go tools at all.

## Build outline

- `src-tauri/src/scan/cargo.rs`, mirroring `scan/npm.rs`'s batch-then-parse shape: read
  `.crates2.json` (JSON, primary) with a `cargo install --list` (text, fallback) parser for when
  the JSON is missing or fails to deserialize. Resolve the install root through
  `CARGO_INSTALL_ROOT` -> `install.root` config -> `CARGO_HOME` -> `$HOME/.cargo`, not
  hardcoded. Classify each `PackageId` key's parenthesized source suffix into
  registry/git/path per design question 3. Fixture tests mirroring `npm.rs`'s
  `current_tool_has_equal_installed_and_latest` / `empty_or_garbage_output_yields_no_rows`
  shapes, plus the multi-binary exclusion fixture from design question 5.
- `Sources.cargo: bool` in `store.rs::Sources` (default `true`, matching every other source's
  default-on convention, `store.rs:20-29`); update `Default for Sources` and extend the existing
  `partial_settings_keeps_other_sources_on` test to cover it.
- Preferences dialog: add `"cargo"` to the `["npm","brew","pip","npx","manual"]` array literal at
  `frontend/index.html:1047`.
- View menu: a `{label:"Source: cargo", checked:..., run:...}` entry alongside the existing five
  at `frontend/index.html:968-972`; the `VIEW.sources` default object at
  `frontend/index.html:347-348` gains `cargo:true`.
- `intel/osv.rs::osv_ecosystem`: add a `"cargo" => Some("crates.io")` arm. This string is
  verified, not assumed: fetched `ossf/osv-schema`'s `ecosystems.json` (lists `"crates.io"`
  exactly) and confirmed live against `api.osv.dev/v1/query` with `{"ecosystem":"crates.io"}`,
  which returned real advisories (RUSTSEC-2020-0071 / GHSA-wcg3-cvx6-7396 for an old `time`
  crate version) this session.
- `ops.rs::build_command`: a `("cargo", "install")` / `("cargo", "update")` arm running
  `cargo install --version <version> <pkg>`, and a `("cargo", "rollback")` arm using the SAME
  command, since cargo genuinely supports installing an arbitrary prior published version (unlike
  brew, which `build_command` already refuses for `"rollback"`, see
  `ops.rs::brew_rollback_is_unsupported`). Add mirrored unit tests plus one asserting cargo
  rollback is NOT unsupported, in contrast to that brew test.
- `search/cargo.rs`: exact-name lookup (crates.io has no free-text search API, closer to pip's
  gap than npm's), sparse-index-first per design question 2, joining `search/mod.rs::search_all`'s
  existing `std::thread::scope` fan-out (`search/mod.rs:50-59`) as a fourth spawned source
  alongside npm/brew/pip.
- A crates.io disclosure line in the frontend, mirroring pip's `exact match` tag
  (`frontend/index.html:456`, `p.eco==="pip"` condition extended to include `"cargo"`), so the
  UI is honest about the same fuzzy-search gap pip already discloses, not a new kind of gap.
- `scan/mod.rs::scan_all` (lines 62-77): add `if sources.cargo { all.extend(cargo::scan_cargo()); }`
  in the same sequential position as npm/brew/pip/npx, before the `other_names` collection, so
  cargo's rows (and per design question 5, every row's underlying `bins`, not just its display
  name) are excluded from the subsequent manual sweep. Note: `scan_all` is sequential today, not
  parallel (see NOTES).
- `manual.rs::managed_roots`: resolve and push cargo's actual install-root `bin/` dir (not only
  the hardcoded `.cargo`-relative entry already at `manual.rs:63`), mirroring the existing
  `npm root -g` / pip user-base shell-outs at `manual.rs:71-86`, to cover a customized
  `CARGO_INSTALL_ROOT`.
- The `scan_all` other-names test and the `manual.rs` custom-root test from design question 5.

## Non-goals

- Go (`go install`). See design question 6.
- Per-binary rows for multi-binary crates. See design question 5; revisit only on specific user
  demand for per-binary pin/rollback granularity.
- A live crates.io description/owners fetch in v1. See design question 4; offline `Cargo.toml`
  read only, matching every other source's v1 scope.
- Bumping the shared `napm` User-Agent string to include contact info. See design question 2;
  flagged as a follow-up that benefits every source, out of scope for a cargo-only change.

## Open questions for the maintainer

- Priority against M11. This document does not argue cargo should jump the queue, only that it
  is cheap relative to what it fixes (a real, structural invisibility bug, not a missing nicety).
- Whether "Source: cargo" should default on or off in `Sources`. Recommended on, matching every
  existing source's default, but cargo users skew toward having MANY installed crates as build
  dependencies transitively touched by `cargo build` (not `cargo install`ed), so it is worth
  confirming `.crates2.json` genuinely only tracks `cargo install` targets and not build-time
  dependency compilation. Everything read this session supports that it does (the `InstallInfo`
  struct and its `bins` field only make sense for installed binaries, and the empty state on this
  build-heavy dev machine, which regularly builds `src-tauri`'s many Rust dependencies, is
  additional real evidence: an empty `.crates2.json` despite a populated `registry/src` and
  `registry/cache` proves the tracker does not conflate "cargo touched this crate while building
  something" with "the user ran cargo install"), but this is worth a second look before shipping,
  not just taking this document's word for it.

## NOTES

- The plan's evidence section frames scan as joining "the parallel scan scope." I read
  `scan/mod.rs::scan_all` directly: it is sequential (`if sources.npm { all.extend(...) }`
  repeated per source, no `std::thread::scope`). Parallelism in this codebase today is in
  `search/mod.rs::search_all` and `intel/osv.rs::scan_security`'s malicious-detail fetch, not in
  `scan_all`. The build outline above reflects the sequential reality; a future perf pass could
  parallelize `scan_all` the way M4.1 parallelized `search_all`, but that is a separate, larger
  change and not assumed here.
- The plan's evidence section says `.crates2.json`'s "source field" records git/path installs. I
  found no such field; the source lives in the `PackageId` map key's parenthesized suffix
  instead. Design question 3 above is written against the corrected, source-verified shape.
- No `cargo install`ed package exists on this machine, so the populated `.crates2.json` and
  `--list` examples in this document are reconstructed from cargo's real serialization/printer
  source code (quoted and cited above), not captured output. This is flagged everywhere it
  applies rather than presented as an on-machine capture, per the instruction not to fabricate.
  The empty-state captures (0-byte tracker files, empty `--list`, exit 0) ARE real captures from
  this machine and are used as such.
