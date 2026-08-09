# Plan 032: Be honest when the installed version is ahead of the registry's

> **Executor instructions**: Follow the plan, run every verification, touch only the in-scope files.
> STOP and report if a STOP condition occurs. Update your row in `plans/README.md` when done.

## Status
- **Priority**: P2 | **Effort**: S-M | **Risk**: LOW
- **Planned at**: `main` @ 2a9508a, 2026-08-08
- **Reported by**: the maintainer, from a screenshot of v0.1.5 with a real library

## Why this matters

A row in the maintainer's library reads:

```
✓   @gsd-build/sdk  [npm]        Installed 1.42.3     Latest 0.1.0
    GSD SDK — programmatic interface for running GSD plans via the Agen…
```

A green ✓ meaning "current", next to a "Latest" that is **fourteen minor versions below** what is
installed. Every part of that row is individually defensible and the row as a whole asserts
something false.

What actually happened: the installed package is private, or its name is shadowed by an unrelated
public package on the registry. The registry's `0.1.0` has nothing to do with the local `1.42.3`.

The backend is already deliberately right about the *status*. `src-tauri/src/scan/version.rs:68-81`
returns `"current"` whenever `latest` is not strictly greater than `installed`, and the doc comment
says exactly why:

> Uses `cmp()`, so a `latest` that is not genuinely newer than `installed` (equal, a downgrade, or
> only different by prerelease/suffix noise) never reads as "update" -- a private or scope-shadowed
> package that reports a lower public "latest" must not show as an available update.

There is even a test asserting it at `version.rs:209` (`status: "current", // downgrade is never an
update`). **Do not change that logic.** Suppressing the false update was correct.

The gap is in the *display*. Having decided the registry number is not comparable, napm still prints
it in a column headed "Latest", which claims it is the latest version of this package. This is the
one thing the project's own rules forbid: CLAUDE.md says do not silently fake what is not
technically possible, surface the limit in the UI; and the maintainer's standing rule is that every
element must be real. A number napm has already concluded is meaningless should not be displayed as
if it were meaningful.

## Current state

`frontend/index.html:437` renders the Latest cell:

```js
h`<td class="${(npx||manual)?'muted':safe?'vernew':held?'verhold':'muted'}">${(npx||manual)?"—":t.latest}</td>`
```

npx and manual rows already get this treatment: they have no meaningful registry latest, so they
print `—` instead of a misleading number. The ahead-of-registry case needs the same honesty.

`statusOf(t)` at **:343** is just `t.status`, the value computed by `status_of` in Rust. Today
`"current"` covers two genuinely different situations that the UI cannot tell apart:
- installed **equals** latest (genuinely up to date), and
- installed is **ahead of** latest (not comparable).

## Approach

Distinguish the two in the backend, then render the second one honestly.

### Step 1: A distinct status from Rust
In `src-tauri/src/scan/version.rs`, extend `status_of` so that when `cmp(latest, installed)` is
`Ordering::Less` it returns a new status (suggested: `"ahead"`) rather than folding into `"current"`.
`Ordering::Equal` stays `"current"`.

Keep the existing behaviour for everything else, including the prerelease/suffix cases: check what
`cmp()` returns for the row at `version.rs:230` (`prerelease of the same version is not an upgrade`)
before you decide, and make sure a prerelease difference does not start reporting as "ahead" if it
should still be "current". State in your report which of the existing test rows changed expectation
and why each change is correct.

**Verify**: `cd src-tauri && cargo test`. Update the table test at `version.rs:154` and add rows for
the new status, including the `1.42.3` vs `0.1.0` case from the report.

### Step 2: Search every consumer of the status string
`"current"` / `"update"` / `"offline"` / `"unmanaged"` are compared as strings in both languages.
A new value that a consumer does not know about will fall through to some default and misrender.

Grep both `src-tauri/src` and `frontend/index.html` for each status literal and enumerate every
site: the `GLYPH` map, `statusRank`, the `VIEW.outdated` filter (**:405**), `safeCount`, the
What's New feed's installed-list construction (**:561**), Update All's eligibility, and the row
renderer. Paste the full list in your report and say what each does with `"ahead"`.

Correct handling: an "ahead" row is **not** outdated, **not** eligible for Update All, and **not**
a What's New card. It behaves like "current" everywhere except display.

### Step 3: Render it honestly
In the Latest cell, an "ahead" row shows `—` (matching how npx/manual already handle "no meaningful
registry latest"), with a `title` tooltip giving the real reason and the actual registry number, in
the app's plain voice. Something like:

> registry publishes 0.1.0, which is older than your 1.42.3; this is usually a private package or a
> name shadowed by an unrelated public one

Give the glyph a matching tooltip so the ✓ is explained rather than merely present. Use `h` /
`esc()` for interpolation exactly as the surrounding code does, and if you build the tooltip
attribute separately follow the `gTitleAttr` pattern at **:426** including its `raw()` wrapper.

Do not invent a new glyph character or colour without checking it against the existing set at
**:101-104**; if ✓ is wrong for this state, say what you picked and why.

### Step 4: House style
No em dashes in any string you add (CLAUDE.md). Note that the existing `.toold` description text can
contain them, since that is upstream package metadata; that is not yours to change.

## Commands
| Purpose | Command | Expected |
|---|---|---|
| Rust tests | `cd src-tauri && cargo test` | all passing, count >= 201 |
| Format | `cd src-tauri && cargo fmt --check` | clean |
| Lint | `cd src-tauri && cargo clippy -- -D warnings` | clean |
| No em dashes added | `git diff main..HEAD \| grep '^+' \| grep '—'` | no matches |

## Scope
**In scope**: `src-tauri/src/scan/version.rs`, the status consumers you enumerate in Step 2,
`frontend/index.html`'s Latest cell and glyph tooltip.
**Out of scope**: `cmp()` itself and its ordering semantics; the What's New recommendation engine;
the library table's layout (that is plan 031); adding a filter or sort for the new status; any
change to what counts as an update.

## Git workflow
- Branch: `advisor/032-ahead-of-registry` from `main`.
- Commit: `fix(ui): do not print a registry version older than the installed one as "Latest"`.
- Do NOT push, open a PR, or merge unless the operator asks.

## Done criteria
- [ ] `status_of` returns a distinct status when installed is ahead of latest; `Ordering::Equal`
      still returns `"current"`
- [ ] Every consumer of the status string handles the new value; the enumeration is in your report
- [ ] An "ahead" row is excluded from outdated counts, Update All, and the What's New feed
- [ ] The Latest cell shows `—` with a tooltip naming the real registry version and the likely cause
- [ ] `cargo test` passing, `cargo fmt --check` clean, `cargo clippy -- -D warnings` clean
- [ ] No em dashes added
- [ ] Only in-scope files changed (`git diff --stat main..HEAD`)

## STOP conditions
- Distinguishing "ahead" from "current" would change which rows count as updates. It must not:
  report and stop.
- The status string turns out to cross the Tauri boundary into persisted state (settings, history,
  or a cache file) where an unknown value would break deserialization on downgrade. Report the
  serialization sites and your proposed migration rather than shipping it blind.

## Maintenance notes
- The same reasoning applies to any future source whose registry lookup can return a version
  unrelated to the installed one. cargo git/path installs are the near neighbour: they already show
  no update path, so check whether they should share this presentation.
- Reviewer: confirm the outdated count in the status bar and the What's New badge did not move for
  rows that were already `"current"`.
