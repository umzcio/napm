# Plan 030: Stop the audit job failing on push over informational advisories

> **Executor instructions**: Follow the plan, run every verification, touch only the in-scope file.
> STOP and report if a STOP condition occurs. Skip updating `plans/README.md`.

## Status
- **Priority**: P1 (main's CI badge is red) | **Effort**: S | **Risk**: LOW | **Category**: dx
- **Planned at**: `main` @ 2a9508a, 2026-08-08

## Why this matters

CI is green on pull requests but red on every push to `main`. The cause is a behavior difference in
`rustsec/audit-check`, not a new vulnerability:

- On `pull_request` it reports only advisories the diff introduces.
- On `push` it scans the whole lockfile and reports everything, **including informational
  advisories** (`unmaintained`, `unsound`), which it then treats as failing.

The firing set is a dozen-plus informational notices, all transitive through Tauri and all in the
GTK/Linux stack that a macOS-only app never executes: `RUSTSEC-2024-0411` through `RUSTSEC-2024-0420`
(the gtk-rs family, deprecated upstream in favor of gtk4), `RUSTSEC-2024-0429` (glib unsoundness),
and `RUSTSEC-2024-0370` (proc-macro-error, unmaintained).

The goal is NOT to silence the job. It must still fail on a real vulnerability. It should stop
failing on "this transitive Linux GUI crate is unmaintained," which is not actionable here and is
already tracked: the glib one was reviewed and dismissed on the GitHub alert with a written reason.

## Current state

`.github/workflows/ci.yml`, `audit` job:
```yaml
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
          working-directory: src-tauri
          # <existing per-id ignore comments>
          ignore: "RUSTSEC-2026-0194,RUSTSEC-2026-0195,RUSTSEC-2026-0235"
```
The three ignored ids are real (non-informational) vulnerabilities previously triaged with reasons in
comments; keep them and keep their comments.

## Commands
| Purpose | Command | Expected |
|---|---|---|
| YAML parses | `python3 -c "import yaml;yaml.safe_load(open('.github/workflows/ci.yml'))"` | no error |
| Local audit (if installed) | `cd src-tauri && cargo audit` | may not be installed; note if so |

## Scope
**In scope**: `.github/workflows/ci.yml` only.
**Out of scope**: upgrading Tauri or any dependency; removing the audit job; disabling it on push;
the three existing per-id ignores; anything under `src-tauri/src`.

## Steps

### Step 1: Pick the mechanism (read the docs first)
Read `rustsec/audit-check`'s README (WebFetch https://github.com/rustsec/audit-check) and determine
the supported way to keep informational advisories from failing the run. Likely candidates, in order
of preference:
1. An input on the action that controls whether warnings/informational advisories fail (e.g. a
   `denyWarnings`-style flag) set so informational notices do not fail the build.
2. Failing that, a `src-tauri/audit.toml` (cargo-audit config) with an `[advisories]` section
   configuring informational handling (`unmaintained`, `unsound`, `yanked`) as warnings rather than
   errors.
3. Only if neither works: add the specific informational ids to the existing `ignore` list, each
   with a one-line reason grouped under a comment explaining they are GTK/Linux transitive
   dependencies unused on macOS.

Whichever you choose, a NEW real vulnerability must still fail the job. State in your report which
mechanism you used and paste the evidence from the docs that it does what you claim.

### Step 2: Apply it, keeping the existing triage intact
Preserve the three existing per-id ignores and their reason comments. Add a short comment explaining
the push-vs-PR behavior difference so the next person does not re-debug it.

**Verify**: the YAML parses; `git diff` touches only the audit job.

## Done criteria
- [ ] `.github/workflows/ci.yml` parses as YAML
- [ ] The three previously-triaged ids remain ignored, with their comments
- [ ] Informational advisories no longer fail the run; a real vulnerability still would (explain how you know)
- [ ] A comment documents the push-vs-pull_request difference
- [ ] Only `.github/workflows/ci.yml` changed (`git diff --stat main..HEAD`)

## STOP conditions
- The only way to make it pass is to disable the audit job or ignore advisories wholesale. Report
  instead; a job that cannot fail is worse than no job.
- Any firing advisory turns out to be a real (non-informational) vulnerability in a crate napm
  depends on directly (`ureq`, `serde`, `serde_json`, `tauri`, `tauri-plugin-*`, `minisign-verify`,
  `log`). That is a genuine finding, not noise: report it.

## Maintenance notes
- These informational notices clear when Tauri moves off the deprecated gtk-rs stack; the ignore or
  config entry should be revisited then, not carried forever.
- Reviewer: confirm the job still fails on a real vulnerability (the mechanism must target
  informational advisories specifically, not severity or the whole job).
