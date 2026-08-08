# M11 research spike: MCP connectors and Claude Code plugins as scanned sources

Status: research only. No source code was written or modified for this spike.

Inspected on this machine:
- Claude Code CLI: `2.1.226`
- Claude Desktop (macOS app): `1.24012.9` (both `CFBundleShortVersionString` and
  `CFBundleVersion`)

All findings below were hand-verified against real config files and live
registry/API calls during this session; the exact commands used are cited
inline so the claims can be re-run rather than taken on faith.

## 1. Config surfaces, with real redacted examples

### 1a. `~/.claude.json` - user-level `mcpServers`

Top-level `mcpServers` (applies everywhere Claude Code runs). Five servers
were configured on this machine. `env` blocks are present but their values
are omitted here; only key names are shown, per this spike's redaction rule.

```json
{
  "google-workspace": {
    "type": "stdio",
    "command": "uvx",
    "args": ["workspace-mcp"]
    // env keys present: GOOGLE_OAUTH_CLIENT_ID, GOOGLE_OAUTH_CLIENT_SECRET
  },
  "ms365": {
    "type": "stdio",
    "command": "npx",
    "args": ["-y", "@pnp/cli-microsoft365-mcp-server@latest"]
  },
  "ms365-softeria": {
    "type": "stdio",
    "command": "npx",
    "args": ["-y", "@softeria/ms-365-mcp-server", "--org-mode"]
    // env keys present: AZURE_CLIENT_ID, AZURE_TENANT_ID
  },
  "openscad": {
    "type": "stdio",
    "command": "uv",
    "args": ["run", "--with", "git+https://github.com/quellant/openscad-mcp.git", "openscad-mcp"]
    // env keys present: OPENSCAD_PATH
  },
  "local-python-server": {
    "type": "stdio",
    "command": "~/mcp-servers/local-python-server/.venv/bin/python",
    "args": ["~/mcp-servers/local-python-server/server.py"]
  }
}
```

### 1b. `~/.claude.json` - per-project `mcpServers`

`~/.claude.json` also carries a `projects` map (169 project entries on this
machine) where individual project paths can each hold their own
`mcpServers` block. This is a **third scope distinct from both the
user-level block above and a real `.mcp.json` file** (section 1c): it is
Claude Code's own project-local record, populated after the user approves a
project-scoped server, and lives inside the single central JSON file rather
than as a separate file at the project root. Six of the 169 projects had a
non-empty `mcpServers` block, for example:

```json
// project ~/Documents/GitHub
"render": {
  "type": "http",
  "url": "https://mcp.render.com/mcp",
  "headers": { "Authorization": "Bearer <redacted>" }
},
"blender": { "type": "stdio", "command": "uvx", "args": ["blender-mcp"] }

// project ~ (the home directory itself)
"notion": { "type": "http", "url": "https://mcp.notion.com/mcp" },
"azure-mcp": {
  "type": "stdio",
  "command": "npx",
  "args": ["-y", "@azure/mcp@latest", "server", "start"]
}
```

Important finding, not anticipated going in: **the `render` and
`example-remote-proxy` entries carry the bearer token as a literal string
inside `headers.Authorization` (or, for one Desktop entry, inside an `args`
string passed to `mcp-remote --header`), not inside a structured `env`
block.** A scanner that only redacts `env` will leak credentials. See
section 4.

### 1c. Claude Desktop's `claude_desktop_config.json`

Present at `~/Library/Application Support/Claude/claude_desktop_config.json`
(a `.bak-<timestamp>` sibling also exists, confirming the app itself
snapshots this file before rewriting it). Top-level keys:
`mcpServers`, `coworkUserFilesPath`, `preferences`. Four servers configured:

```json
{
  "project-a-mcp": {
    "command": "node",
    "args": ["~/GitHub/project-a-mcp/dist/index.js"]
    // env keys present: PROJECT_A_BASE_URL, PROJECT_A_API_KEY
  },
  "project-b-mcp": {
    "command": "node",
    "args": ["~/Downloads/project-b-mcp/dist/index.js"]
    // env keys present: PROJECT_B_API_URL, PROJECT_B_API_KEY
  },
  "example-remote-proxy": {
    "command": "npx",
    "args": ["-y", "mcp-remote", "https://example-remote.example.com/api/mcp",
              "--header", "Authorization: Bearer <redacted>"]
  },
  "local-python-server": {
    "command": "~/mcp-servers/local-python-server/.venv/bin/python",
    "args": ["~/mcp-servers/local-python-server/server.py"]
  }
}
```

Desktop has no project concept, so this file is the entire Desktop-side
surface; there is no per-project equivalent of 1b for Desktop.

### 1d. Project `.mcp.json` files

Real examples found via a bounded search of `~/`, `~/Documents/GitHub`,
`~/GitHub`, and `~/Downloads` (maxdepth 3, not a full-disk trawl):

```json
// ~/.mcp.json
{ "mcpServers": { "example-remote-mcp": {
    "type": "http",
    "url": "https://mcp.example.com/mcp?project_ref=<redacted>"
} } }

// ~/GitHub/project-c/.mcp.json
{ "mcpServers": { "example-remote-mcp": {
    "type": "http",
    "url": "https://mcp.example.com/mcp?project_ref=<redacted>"
} } }
```

(Both real examples used a hosted-backend-as-a-service MCP server, i.e. one
server per project pointing at that project's own backend instance via a
`project_ref`-style query parameter; the service name and both `project_ref`
values are redacted above since a `project_ref` is an identifying
infrastructure value, not just a credential-adjacent one.) Both are minimal:
a single remote HTTP MCP server, no local install, and the general lesson
holds regardless of which specific service is involved: a scanner should
treat unfamiliar query-string content as untrusted and not echo it verbatim
without consideration, even when the individual value is not, strictly
speaking, a credential.

### 1e. `~/.claude/plugins/` layout

```
~/.claude/plugins/
  installed_plugins.json      # the source of truth for what's installed + its version
  known_marketplaces.json     # marketplace name -> git source + last sync time
  blocklist.json
  plugin-catalog-cache.json   # 407k, browse-time catalog cache, not install state
  cache/<marketplace>/<plugin>/<version-or-"unknown">/   # actual installed files
  marketplaces/<marketplace>/  # a checked-out copy of the marketplace repo itself
```

`installed_plugins.json` (`version: 2`) is a flat map of
`"<plugin>@<marketplace>"` to an array of install records:

```json
"swift-lsp@claude-plugins-official": [{
  "scope": "user",
  "installPath": "~/.claude/plugins/cache/claude-plugins-official/swift-lsp/1.0.0",
  "version": "1.0.0",
  "installedAt": "2026-01-20T00:47:01.276Z",
  "lastUpdated": "2026-01-20T00:47:01.276Z",
  "gitCommitSha": "96276205880a60fd66bbae981f5ab568e70c4cbf"
}],
"superpowers@claude-plugins-official": [{
  "scope": "user",
  "installPath": "~/.claude/plugins/cache/claude-plugins-official/superpowers/6.2.0",
  "version": "6.2.0",
  "installedAt": "2026-04-11T18:15:31.482Z",
  "lastUpdated": "2026-07-24T22:59:11.336Z",
  "gitCommitSha": "917e5f53b16b115b70a3a355ed5f4993b9f8b73d"
}],
"frontend-design@claude-plugins-official": [{
  "scope": "user",
  "installPath": "~/.claude-team/plugins/cache/claude-plugins-official/frontend-design/unknown",
  "version": "unknown",
  "installedAt": "2026-04-11T18:13:40.316Z",
  "lastUpdated": "2026-08-08T04:47:57.486Z"
}]
```

Two findings not anticipated by the roadmap text:

1. **Plugins are not all under one root.** `frontend-design`, `playwright`,
   `code-review`, and `skill-creator` are installed under a second,
   completely separate directory, `~/.claude-team/plugins/cache/...`
   (confirmed by listing that directory directly), not
   `~/.claude/plugins/cache/...`. A scanner that hardcodes the single
   `~/.claude/plugins` root will silently miss these. The mechanism that
   selects the alternate root was not investigated further (out of scope
   for this spike) but must be accounted for before `scan/mcp.rs` ships.
2. **`"version": "unknown"` is literal, not a scanner artifact.** Reading
   the file at
   `~/.claude-team/plugins/cache/claude-plugins-official/frontend-design/unknown/.claude-plugin/plugin.json`
   directly shows it has no `version` field at all:
   ```json
   { "name": "frontend-design", "description": "...", "author": {...} }
   ```
   Claude Code itself could not determine a version for this plugin and
   recorded the string `"unknown"`. This independently confirms the
   roadmap's honesty rule from a source outside napm entirely: when even
   the upstream tool cannot resolve a version, napm must show "unmanaged",
   never fabricate one.

`known_marketplaces.json` records each marketplace's git source and last
sync time:

```json
{
  "claude-plugins-official": {
    "source": { "source": "github", "repo": "anthropics/claude-plugins-official" },
    "installLocation": "~/.claude/plugins/marketplaces/claude-plugins-official",
    "lastUpdated": "2026-08-07T17:13:58.897Z"
  },
  "openai-codex": {
    "source": { "source": "github", "repo": "openai/codex-plugin-cc" },
    "installLocation": "~/.claude/plugins/marketplaces/openai-codex",
    "lastUpdated": "2026-07-25T15:21:32.514Z"
  }
}
```

The marketplace's own manifest (`.claude-plugin/marketplace.json` inside the
checked-out marketplace repo) lists each plugin's declared source. Two very
different shapes exist side by side in the *same* marketplace:

```json
// bundled-in-repo plugin: source is a relative path, version lives in its own plugin.json
{ "name": "swift-lsp", "version": "1.0.0", "source": "./plugins/swift-lsp", ... }

// externally-hosted plugin: source is a pinned git commit, no version field here at all
{ "name": "superpowers", "source": {
    "source": "url", "url": "https://github.com/obra/superpowers.git",
    "sha": "44c9b2d6e889982ac18c27d05a19fefe335194e1"
} }
```

## 2. Install-type classification

| Install type | Where "installed version" is recorded | Resolvable "latest"? | Already covered by existing napm scanners? |
|---|---|---|---|
| npx-invoked npm package, config-pinned to a version or `@latest` (`azure-mcp`, `ms365`, `ms365-softeria`, `example-remote-proxy`'s `mcp-remote`) | The npx cache (`~/.npm/_npx/<hash>/node_modules/<pkg>/package.json`), **if and only if that exact spec has actually been run at least once** - config alone does not record a version | Yes, `dist-tags.latest` from the npm registry (identical mechanism to `scan/npx.rs`) | Package identity + cached version: yes, via `scan/npx.rs`. Connector wiring (which config file references it, under what server name, with what secrets) is genuinely new. |
| `uvx <pypi-name>` / `uv run --with <pypi-name>` (`google-workspace`'s `workspace-mcp`, `blender-mcp`) | **Nowhere locally**, confirmed live: `uv tool list` reports no tools installed (these are ephemeral `uvx`/`uv run` invocations, not `uv tool install`), and `uv`'s own cache (`~/.cache/uv/environments-v2`) is keyed by opaque hashes, not a readable package name, so there is no clean walk analogous to npx's `_npx.packages` shim | Yes for the PyPI name itself (exact-name PyPI lookup, same mechanism M4 already uses for pip), but with **no local "installed" version to diff against** | Not covered at all today; this is a new, harder case than npx because there is no cheap local version signal. |
| `uv run --with git+<url>` (`openscad-mcp`) | Nowhere locally, same as above, and there is no PyPI entry to check either (it isn't a PyPI package name at all) | No. No registry, no dist-tag, nothing to compare against except the git repo's own commit history | Not covered; correctly "unmanaged." |
| Raw absolute path to an interpreter + script (`local-python-server` in both `~/.claude.json` and Desktop's config; Desktop's `project-a-mcp`, `project-b-mcp` pointing at `node .../dist/index.js`) | Only if the target directory happens to have its own `package.json`/version file, which is a coincidence of that particular project, not something the MCP config guarantees | No general mechanism; would require locating and trusting an arbitrary local repo's own manifest | Closest existing analog is `scan/manual.rs`'s philosophy (resolve if trivially possible, else blank), but manual.rs walks `$PATH` executables, not arbitrary config-referenced paths - this needs new code, not reuse. |
| Remote HTTP MCP (`render`, `notion`, `example-remote-mcp` in both `.mcp.json` files) | N/A - there is no local install, it's a hosted endpoint referenced by URL | No meaningful "version" concept locally; a remote service can change under the user with no local signal at all | Entirely new territory; "latest" does not apply, only "is this URL/token still valid" would, which is out of scope for a version-freshness scanner. |
| Claude Code plugin, bundled in marketplace repo, own semver (`swift-lsp`) | `installed_plugins.json`'s `version` field, cross-checked against the plugin's own `.claude-plugin/plugin.json` `version` field (both said `1.0.0`, hand-verified match) | Yes, by re-syncing the marketplace repo and comparing its listed `version` for that plugin | New; no existing scanner touches plugins. |
| Claude Code plugin, externally hosted git repo, no semver tracked (`superpowers`) | `installed_plugins.json`'s `gitCommitSha`, **not** its `version` field (see section 3 - the version field proved unreliable) | Yes, but only via commit comparison: `known_marketplaces.json`'s pinned `source.sha` after a marketplace sync, diffed against the installed `gitCommitSha` | New. |
| Claude Code plugin under the alternate `~/.claude-team/` root, no version resolvable at all (`frontend-design`, `playwright`, `code-review`, `skill-creator`) | Nowhere - `installed_plugins.json` literally stores `"version": "unknown"`, and the plugin's own manifest has no `version` field either (confirmed by reading it directly) | No | New, and must be labeled "unmanaged" exactly as M9 does - this is the clearest, most directly-confirmed honesty case in the whole spike. |

## 3. Version truth-testing (hand-verified)

### 3a. npx-style server: `@azure/mcp`

Configured in `~/.claude.json` (project `~`, the home directory itself) as
`npx -y @azure/mcp@latest server start`. The local npx cache
(`~/.npm/_npx/<hash>/`) had actually run this spec before and recorded a
cached install:

- Cached installed version (from
  `~/.npm/_npx/<hash>/node_modules/@azure/mcp/package.json`):
  `3.0.0-beta.31`
- Live registry `dist-tags.latest`
  (`curl -s https://registry.npmjs.org/@azure/mcp` ->
  `.dist-tags.latest`): `3.0.0-beta.33`

**Real drift confirmed**: the cached/last-run version is two beta releases
behind the registry's current latest, exactly the kind of staleness the M11
feature is meant to surface. This is the same mechanism as the existing
"npx latest-drift hint" (`scan/npx.rs` + the deferred `npx_latest` lookup
described in the roadmap's M10b section), so MCP servers invoked via bare
`npx` ride that mechanism for free once the connector layer knows to look at
this cache entry.

A second npx-style server, `@softeria/ms-365-mcp-server`, was also checked
for completeness: cached `0.137.0` vs. registry latest `0.137.0` - no drift,
confirming the check also correctly reports "no update" rather than always
finding a difference.

### 3b. Plugin: `superpowers`

- `installed_plugins.json` records `version: "6.2.0"`, `gitCommitSha:
  917e5f53b16b115b70a3a355ed5f4993b9f8b73d`, `lastUpdated:
  2026-07-24T22:59:11.336Z`.
- `known_marketplaces.json` shows the marketplace itself was last synced
  `2026-08-07T17:13:58.897Z`, and the marketplace manifest pins superpowers
  to commit `44c9b2d6e889982ac18c27d05a19fefe335194e1`.
- Queried the GitHub commits API directly for both SHAs
  (`api.github.com/repos/obra/superpowers/commits/<sha>`):
  - installed commit `917e5f53b16b` -> committer date `2026-04-06T22:48:58Z`
  - marketplace-pinned commit `44c9b2d6e889` -> committer date
    `2026-07-28T19:25:36Z`

**Real update confirmed available** (installed commit is roughly 3.5 months
older than the marketplace's current pin), and it is invisible from the
`version` field alone. Fetching `plugin.json` at the marketplace-pinned
commit
(`raw.githubusercontent.com/obra/superpowers/44c9b2d.../.claude-plugin/plugin.json`)
shows `"version": "6.2.0"` - **the same string as the installed copy**,
despite months and (per the diverging commit history) real intervening
changes. The author simply never bumped the field between these two points.

**Conclusion for the scanner design**: for git-url-sourced plugins, the
self-reported `version` string cannot be trusted as a freshness signal at
all. The only reliable check is commit SHA comparison (installed
`gitCommitSha` vs. the marketplace's currently-pinned `source.sha`, after a
marketplace re-sync), the same "compare an opaque identifier, don't trust a
self-reported label" posture napm already takes with npm's `dist-tags` vs. a
package's own claims.

## 4. Redaction: field and pattern list for a future `scan/mcp.rs`

No secret values are reproduced anywhere in this document. A future scanner
must redact more than the obvious `env` block:

- **`env` object values**, any key. Seen on this machine (names only, not
  values): `GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_OAUTH_CLIENT_SECRET`,
  `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `OPENSCAD_PATH`,
  `UAPPROVE_BASE_URL`, `UAPPROVE_API_KEY`, `UMSTATUS_API_URL`,
  `UMSTATUS_API_KEY`. Not all of these are secrets by name (a `_PATH` or
  `_URL` suffix is often just config), but the safe rule is to redact every
  `env` value unconditionally and show only the key name, since a scanner
  cannot know per-deployment which env vars are sensitive.
- **`headers` object values**, especially `Authorization`. Found live on
  this machine: a bearer token embedded directly in a `headers.Authorization`
  string for an HTTP-type server, structurally identical to an env secret
  but living in a completely different part of the schema.
- **Secrets smuggled inside a plain `args` string**, not a structured field
  at all. Found live: `mcp-remote <url> --header "Authorization: Bearer
  <token>"` passes the credential as a literal array element. A key-name
  allowlist approach (redact `env`, `headers`) misses this entirely; the
  scanner needs a pattern-based pass over every string value in `args`
  (`Bearer `, `Authorization:`, `token=`, `key=`, `apikey=`, `secret=`,
  and common vendor prefixes like `sk-`, `ghp_`, `rnd_`) in addition to the
  structural redaction.
- **URL query strings** can carry identifiers that are not strictly secrets
  but are still not for display (e.g. a `project_ref` parameter uniquely
  identifying a hosted project). Treat unfamiliar query-string content as
  untrusted by default rather than echoing it verbatim in any UI.

This spike's own tooling had to be corrected mid-investigation: an initial
extraction script filtered out only `env` and missed the `headers`-embedded
and `args`-embedded tokens, which appeared in raw form in an intermediate
shell result before being caught and excluded from every subsequent output
and from this document. That near-miss is itself evidence for the pattern-
based redaction pass above; key-name filtering alone is not sufficient.

**Note on injected instructions:** several config and cache files inspected
for this spike (personal MCP configs, plugin catalog cache) contain
free-text fields (descriptions, plugin metadata) that could in principle
attempt to instruct an agent reading them. None of the files read during
this spike contained anything that read as an attempted instruction to this
agent; this is recorded per the task's standing instruction to note the
observation either way.

## 5. Proposed `InstalledTool` row mapping

Current struct (`src-tauri/src/scan/mod.rs:14-36`):

```rust
pub struct InstalledTool {
    pub name: String,
    pub eco: String,
    pub pkg: String,
    pub installed: Option<String>,
    pub latest: String,
    pub size: String,
    pub pinned: bool,
    pub publisher: String,
    pub description: String,
    pub updated: i64,
    pub requested: bool,
}
```

Proposed mapping for an `mcp` source (fields not listed follow the existing
convention: `size` = 0/"" since a connector has no on-disk footprint of its
own beyond its backing package; `updated` = config file mtime; `requested`
= always `true`, a connector only exists because the user wired it up):

| Field | MCP connector value |
|---|---|
| `name` | the server name as configured (`"azure-mcp"`, `"superpowers"`, etc.) |
| `eco` | `"mcp"` for connectors, `"plugin"` for `~/.claude/plugins` entries (two sub-kinds, not one, since their install/version story genuinely differs - see section 2) |
| `pkg` | for npx/uvx-backed servers, the backing package name (`"@azure/mcp"`); for plugins, `"<plugin>@<marketplace>"`; for remote/raw-path servers, the server name itself since there is no package identity |
| `installed` | the resolved version from section 2/3 where one exists; `None` for remote HTTP servers, raw-path scripts without a discoverable manifest, and git-sourced uv/uvx servers |
| `latest` | resolved dist-tag/PyPI version, or marketplace-pinned SHA/version for plugins; **equal to `installed` (i.e. "no known update") is not always honest here** - for the `superpowers`-style case, `latest` must come from SHA comparison, not the `version` field, or it will silently under-report drift the way the field itself does |
| `publisher` | npm/PyPI author where resolvable (existing `publisher.rs` logic applies to the backing package); plugin `author.name` from its manifest; blank for remote/raw-path |
| `description` | server's role or plugin description where available |
| `size` | not meaningful for a connector; leave `"n/a"` or empty rather than reusing the backing package's own `size` (that already appears on that package's own npm/pip row if it happens to also be globally installed) |

Status derivation needs a **new fourth state** beyond current/update/not-
installed: **unmanaged** (mirrors `scan/manual.rs`'s badge), used whenever
section 2's table says "no resolvable latest" or "no locally recorded
version" - remote HTTP servers, git-url uv/uvx servers, raw-path scripts
without a manifest, and any plugin whose own `version` is literally
`"unknown"`.

## 6. Effort estimate for `scan/mcp.rs` behind a `Sources` flag

Split into two genuinely separate pieces given how different their install-
type stories are (section 2):

- **Connector config layer** (parse `~/.claude.json` both scopes, Desktop's
  `claude_desktop_config.json`, and discovered `.mcp.json` files; classify
  by `command`/`type`; resolve npx-backed versions by reusing
  `scan/npx.rs`'s cache-walk almost as-is; add the npm-dist-tag / PyPI-
  exact-name "latest" lookups, largely reusing M4's existing HTTP layer):
  **roughly 1.5-2 days**, dominated by the redaction pass (section 4) and
  correctly discovering `.mcp.json` files without a full-disk walk (the
  roadmap explicitly does not want a full trawl; needs a bounded, sensible
  search strategy - project list from `~/.claude.json`'s own `projects` map
  is a reasonable seed).
- **Plugin layer** (parse `installed_plugins.json` and
  `known_marketplaces.json`; semver path via each plugin's own `plugin.json`
  for bundled-source plugins; SHA-comparison path for git-url-sourced
  plugins, requiring a marketplace re-sync step or a live GitHub API commit
  lookup; discover the alternate `~/.claude-team/`-style root(s) rather than
  hardcoding one): **roughly 1-1.5 days**, with the multi-root discovery and
  the SHA-comparison path being the two genuine unknowns (this spike
  verified the SHA-comparison mechanism works, but did not investigate how
  Claude Code itself decides to use a second plugins root, which needs a
  short look before committing to a fixed set of root paths).

Total: **roughly 3 days** for a v1 `scan/mcp.rs` covering both connectors
and plugins with honest `unmanaged` fallback, excluding UI wiring (a new
source toggle, row rendering for the two new `eco` values, and any
"jump to config file" action, which are normal M-sized UI work on top).

## 7. Open product question

The roadmap already flags this: **fifth-and-sixth library rows vs. a
separate view.** Evidence from this spike bears on it both ways:

- *For folding into the existing library rows* (as a `mcp`/`plugin` `eco`,
  same as M9 added `manual`): connectors and plugins share the same core
  question the rest of the library already answers ("what do I have, is it
  current"), and the existing View-menu per-source toggles and `Sources`
  flag plumbing (M6/M7) already generalize cleanly to two more sources with
  no new UI surface needed.
- *For a separate view*: connectors are not really "installed software" in
  the way npm/brew/pip/npx/manual rows are - several have **no local
  install at all** (remote HTTP servers), and the meaningful unit for a
  connector is "which config file wires it up and with what secrets,"
  which does not map naturally onto columns built for package/version/size.
  A dedicated "Connectors" view could show the config surface (which file,
  which scope, redacted args/env) as first-class information, which the
  Shared Library's row shape has no room for today.

This spike does not resolve the question; it surfaces the concrete tension
so the milestone owner can decide with real examples in hand rather than in
the abstract.
