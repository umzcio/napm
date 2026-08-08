# Plan 018: Disclose what napm sends where, and make advisory checks a labeled choice

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat bb85e05..HEAD -- src-tauri/src/intel/ src-tauri/src/store.rs frontend/index.html README.md`
> Plans 007/008/010/012/014 legitimately touch these; reconcile against their
> diffs. Any other mismatch with the excerpts below is a STOP condition.

## Status

- **Priority**: P3
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: security / docs
- **Planned at**: commit `bb85e05`, 2026-08-07

## Why this matters

On every launch, napm POSTs the user's complete installed-package inventory — every npm/npx/pip package name with its exact version — to OSV.dev, and resolves per-package metadata against npm and PyPI. This is inherent to the security feature, over TLS, to reputable services; it is not a defect. But a versioned software inventory is a fingerprint and a vulnerability roadmap for the machine, and the app sends it automatically with no disclosure anywhere: not in the About dialog, not in the README, not in Preferences. The fix is a disclosure sentence where users look, plus an advisory-check toggle that fails CLOSED (off must render as "checks disabled", never as an all-clear) — the codebase already models exactly that failure-honesty with `security_ok`.

## Current state

- `src-tauri/src/intel/osv.rs:57-82` — `scan_security` builds the batch (`{package:{ecosystem,name},version}` per installed tool) and POSTs to `https://api.osv.dev/v1/querybatch`.
- `src-tauri/src/intel/mod.rs:92-95` — `whats_new` runs it on every feed load; the frontend triggers that after every scan (`frontend/index.html:866`).
- Other automatic per-package network destinations (for the disclosure sentence): registry.npmjs.org and pypi.org (release verdicts, npx drift, changelogs — `intel/release.rs`, `lib.rs:141-150`), formulae.brew.sh (search catalog + analytics), api.github.com (wire advisories, changelogs, issue-velocity; with the user's token when configured).
- `frontend/index.html:1066-1080` area — the About modal (`showAbout`; locate with `grep -n "showAbout" frontend/index.html`) — the natural disclosure surface.
- `README.md` — no data-handling statement (verify: `grep -in "osv" README.md` shows feature mentions only).
- The honesty machinery to reuse: `WhatsNew.security_ok` (`intel/mod.rs:69-75`), rendered at `frontend/index.html:575-577` as "check unavailable ... not an all-clear".
- Settings shape: `store.rs:32-37` (`Settings`, serde camelCase + default) — plan 014 adds a field the same way; follow the same pattern for `advisory_checks: bool` default true.
- Preferences UI and save path: `frontend/index.html:1045-1056`, `:1085-1093`.
- UI copy rules: no em dashes; honest limits, never fake all-clears.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Tests | `cd src-tauri && cargo test` | exit 0 |
| Run the app | `npm run tauri dev` | checklist below |

## Scope

**In scope**:
- `store.rs` (one `Settings` field)
- `src-tauri/src/intel/mod.rs` (honor the setting; a distinct disabled state)
- `src-tauri/src/lib.rs` (pass the setting into `whats_new`)
- `frontend/index.html` (About text, Preferences toggle, disabled-state rendering)
- `README.md` (one short data-handling paragraph)

**Out of scope**:
- Any change to WHAT is sent when checks are on.
- Telemetry/analytics of any kind (none exists; keep it that way).
- The GitHub-token flows (already user-configured and disclosed by their own labels).

## Git workflow

- Branch: `advisor/018-disclosure`
- Commits: `docs: data-handling disclosure in About and README` and `feat(prefs): advisory checks toggle that fails closed`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 1: The disclosure text

Add to the About modal body and, near-verbatim, as a short README subsection ("What napm sends"): "To check your tools, napm sends package names and versions to OSV.dev (advisory scan), registry.npmjs.org and pypi.org (versions and changelogs), formulae.brew.sh (the brew catalog), and api.github.com (release notes and the supply-chain wire, with your token if you set one). Nothing else leaves your machine, and nothing is sent anywhere else." Adjust phrasing to the file's voice; commas and colons, no em dashes. Keep it accurate against the destinations listed in Current state — if a later plan added a destination, include it.

### Step 2: The setting

- `store.rs`: add `pub advisory_checks: bool` to `Settings`, default true (same serde pattern as the existing fields; old settings files must parse with it true — add the deserialization test).
- `lib.rs` `get_whats_new`: read the setting via `open_store(&app).settings()` and pass it into `intel::whats_new` (new parameter `advisories_enabled: bool`).
- `intel/mod.rs`: when disabled, skip the OSV spawn entirely and set a NEW output field `security_disabled: bool` on `WhatsNew` (serialized camelCase like its siblings), with `security_ok: false`. The wire and verdicts still run (they are per-package metadata lookups the user can reason about; only the batched inventory scan is behind this toggle — state that in the toggle's label).

### Step 3: Preferences + rendering

- `renderPrefs`: checkbox "Scan installed tools against the OSV advisory database", checked from `s.advisoryChecks!==false`; save path writes it.
- `renderFeed`: when the payload has `securityDisabled`, render a distinct signal line: "advisory scan is off in Preferences, so this is not an all-clear" (reuse the `signal danger` styling of the existing `security_ok` branch at `:575-577`, but the distinct wording — "off by choice" must not read as "check broke").

**Verify** (app run): default on → feed unchanged, About shows the disclosure. Toggle off in Preferences → What's New shows the "off in Preferences" line and no OSV request occurs (confirm via the absence of alerts and, if easy, a proxy/console observation — otherwise code-read the skip path). Toggle back on → alerts return.

## Test plan

- `store.rs`: old settings JSON (no `advisoryChecks`) deserializes with `advisory_checks == true`.
- `intel/mod.rs`: `whats_new` with `advisories_enabled=false` returns `security_ok=false`, `security_disabled=true`, empty alerts, and does not call OSV (the OSV call is inside the skipped spawn; assert on the output shape).
- Manual checklist in Step 3.

**Verification**: `cd src-tauri && cargo test` → exit 0.

## Done criteria

- [ ] `cd src-tauri && cargo test` exits 0; the settings-default and disabled-shape tests pass
- [ ] About modal and README contain the disclosure; `grep -in "osv.dev" README.md` → present
- [ ] Preferences toggle exists; off renders the distinct "off in Preferences, not an all-clear" line
- [ ] Old settings files parse with checks enabled
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts do not match beyond the named plans' expected diffs.
- Adding a field to `WhatsNew` breaks the frontend's payload handling in a way that needs more than reading the new field (it must not — the frontend reads fields defensively with `r.x!==false` patterns).

## Maintenance notes

- If a new automatic network destination is ever added (e.g. crates.io via plan 022), the disclosure sentence must be updated in the same PR — reviewers should treat a new `http::get` host as a docs-touching change.
- The toggle deliberately does NOT cover per-package registry lookups; if a fully-offline mode is ever wanted, that is a bigger feature (label it honestly: no verdicts, no drift hints).
