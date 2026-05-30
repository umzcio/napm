#!/usr/bin/env node
"use strict";

/*
 * napm  ♪  a Napster-era package-update tracker
 * Part of the TermWire cinematic universe.
 *
 * Your installed dev CLIs, rendered as files on a P2P network.
 * An "up arrow" means some peer is sharing a newer release.
 *
 * Usage:
 *   node napm.js              scan your system and show the dashboard
 *   node napm.js --demo       fake data, no system probing (great for screenshots)
 *   node napm.js --update     download (npm i -g) everything that's outdated
 *   node napm.js --no-splash  skip the dial-up connect animation
 */

const { execSync } = require("child_process");

// ---------------------------------------------------------------------------
// MANIFEST  — add/rename tools here. `eco` selects the detection strategy.
// `user` is pure Napster flavor: who's "sharing" this file on the network.
// ---------------------------------------------------------------------------
const TOOLS = [
  { name: "Claude Code", eco: "npm", pkg: "@anthropic-ai/claude-code", user: "anthr0pic_official" },
  { name: "Gemini CLI",  eco: "npm", pkg: "@google/gemini-cli",        user: "g00gle_guy_2001"   },
  { name: "Codex CLI",   eco: "npm", pkg: "@openai/codex",             user: "uncle_sam_altman"  },
  { name: "TypeScript",  eco: "npm", pkg: "typescript",                user: "type_daddy"        },
  { name: "Vercel",      eco: "npm", pkg: "vercel",                    user: "edge_lord_69"      },
  // brew / pip examples — uncomment if you use them:
  // { name: "ripgrep", eco: "brew", pkg: "ripgrep", user: "blazng_fast" },
  // { name: "httpie",  eco: "pip",  pkg: "httpie",  user: "http_head"   },
];

// ---------------------------------------------------------------------------
const C = {
  reset: "\x1b[0m", dim: "\x1b[2m", bold: "\x1b[1m",
  red: "\x1b[31m", green: "\x1b[32m", yellow: "\x1b[33m",
  blue: "\x1b[34m", magenta: "\x1b[35m", cyan: "\x1b[36m", gray: "\x1b[90m",
};
const paint = (s, c) => `${c}${s}${C.reset}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function sh(cmd) {
  return execSync(cmd, { stdio: ["ignore", "pipe", "ignore"], encoding: "utf8" });
}
function shAllowFail(cmd) {
  try { return sh(cmd); }
  catch (e) { return e.stdout ? e.stdout.toString() : ""; }
}
const tryJSON = (s) => { try { return JSON.parse(s); } catch { return null; } };

// deterministic fake ping so a tool always "feels" the same
function ping(name) {
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return 8 + (h % 320);
}

// ---------------------------------------------------------------------------
// Ecosystem scanners — each returns Map<pkg, {current, latest}>
// ---------------------------------------------------------------------------
const _cache = {};
function scan(eco) {
  if (_cache[eco]) return _cache[eco];
  const fn = { npm: scanNpm, brew: scanBrew, pip: scanPip }[eco];
  return (_cache[eco] = fn ? fn() : new Map());
}

function scanNpm() {
  const map = new Map();
  const ls = tryJSON(shAllowFail("npm ls -g --depth=0 --json"));
  for (const [pkg, info] of Object.entries(ls?.dependencies || {})) {
    if (info?.version) map.set(pkg, { current: info.version, latest: info.version });
  }
  const od = tryJSON(shAllowFail("npm outdated -g --json")) || {};
  for (const [pkg, info] of Object.entries(od)) {
    map.set(pkg, { current: info.current || map.get(pkg)?.current || "?", latest: info.latest });
  }
  return map;
}

function scanBrew() {
  const map = new Map();
  shAllowFail("brew list --versions").trim().split("\n").forEach((line) => {
    const parts = line.trim().split(/\s+/);
    const name = parts.shift();
    if (name) map.set(name, { current: parts.pop(), latest: parts[parts.length - 1] });
  });
  const od = tryJSON(shAllowFail("brew outdated --json=v2"));
  (od?.formulae || []).forEach((f) =>
    map.set(f.name, { current: (f.installed_versions || []).slice(-1)[0], latest: f.current_version })
  );
  return map;
}

function scanPip() {
  const map = new Map();
  (tryJSON(shAllowFail("pip list --format=json")) || []).forEach((p) =>
    map.set(p.name.toLowerCase(), { current: p.version, latest: p.version })
  );
  (tryJSON(shAllowFail("pip list --outdated --format=json")) || []).forEach((p) =>
    map.set(p.name.toLowerCase(), { current: p.version, latest: p.latest_version })
  );
  return map;
}

// ---------------------------------------------------------------------------
// Demo data
// ---------------------------------------------------------------------------
const DEMO = {
  "@anthropic-ai/claude-code": { current: "1.2.3",  latest: "1.4.0"  },
  "@google/gemini-cli":        { current: "0.9.1",  latest: "0.9.1"  },
  "@openai/codex":             { current: "0.21.0", latest: "0.24.2" },
  "typescript":                { current: "5.5.4",  latest: "5.6.2"  },
  "vercel":                    null, // not installed → offline peer
};

// ---------------------------------------------------------------------------
// Resolve every tool to a row
// ---------------------------------------------------------------------------
function resolve(tool, demo) {
  const key = tool.eco === "pip" ? tool.pkg.toLowerCase() : tool.pkg;
  const entry = demo ? DEMO[tool.pkg] : scan(tool.eco).get(key);
  if (!entry) return { ...tool, status: "offline" };
  const status = entry.current !== entry.latest ? "update" : "current";
  return { ...tool, ...entry, status };
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------
function banner() {
  console.log(paint(" ╔══════════════════════════════════════════════════════════════╗", C.magenta));
  console.log(paint(" ║ ", C.magenta) + paint("napm", C.bold + C.cyan) +
    paint("   ♪ the package-sharing network for your command line   ", C.dim) + paint("║", C.magenta));
  console.log(paint(" ╚══════════════════════════════════════════════════════════════╝", C.magenta));
}

const GLYPH = {
  update:  paint("↑", C.yellow),
  current: paint("✓", C.green),
  offline: paint("✗", C.gray),
};

function table(rows) {
  const W = [16, 11, 11, 20, 6]; // TOOL INSTALLED LATEST SHAREDBY PING
  const head = ["TOOL", "INSTALLED", "LATEST", "SHARED BY", "PING"];
  const sep = paint(" │ ", C.gray);
  const line = (cells, colors) =>
    cells.map((c, i) => {
      const padded = String(c ?? "").padEnd(W[i]);
      return colors && colors[i] ? paint(padded, colors[i]) : padded;
    }).join(sep);

  const totalW = W.reduce((a, b) => a + b, 0) + (W.length - 1) * 3;
  console.log("   " + line(head, head.map(() => C.dim + C.bold)));
  console.log("   " + paint("─".repeat(totalW), C.gray));

  for (const r of rows) {
    const installed = r.status === "offline" ? "—"      : r.current;
    const latest    = r.status === "offline" ? "—"      : r.latest;
    const pingStr   = r.status === "offline" ? "----"   : `${ping(r.name)}ms`;
    const colors = [
      r.status === "update" ? C.bold + C.yellow : r.status === "offline" ? C.gray : C.reset,
      C.dim,
      r.status === "update" ? C.bold + C.green : C.dim,
      C.magenta,
      C.dim,
    ];
    console.log(" " + GLYPH[r.status] + " " +
      line([r.name, installed, latest, "@" + r.user, pingStr], colors));
  }
}

function statusBar(rows) {
  const need = rows.filter((r) => r.status === "update").length;
  const online = rows.filter((r) => r.status !== "offline").length;
  const bar = "█".repeat(6) + "░".repeat(4);
  console.log();
  console.log(" " + paint("●", C.green) + paint(" Connected (56.6 kbps)", C.dim) +
    paint("   │   ", C.gray) +
    paint(`${need} new release${need === 1 ? "" : "s"} available`, need ? C.yellow : C.dim) +
    paint("   │   ", C.gray) +
    paint(`${online}/${rows.length} peers sharing`, C.dim));
  console.log(" " + paint(`throttle: [${bar}] 56k`, C.gray) + paint("  (does nothing)", C.dim));
  if (need) console.log("\n " + paint(`run `, C.dim) + paint("napm --update", C.cyan) +
    paint(` to download all ${need} update${need === 1 ? "" : "s"}`, C.dim));
}

// ---------------------------------------------------------------------------
// --update: fake progress bar, then a real `npm i -g pkg@latest`
// ---------------------------------------------------------------------------
async function downloadBar(label) {
  const width = 24;
  for (let i = 0; i <= width; i++) {
    const bar = "█".repeat(i) + "░".repeat(width - i);
    const kb = Math.round((i / width) * (180 + (ping(label) % 400)));
    process.stdout.write(`\r   ${paint(bar, C.cyan)} ${kb}KB  ${Math.round((i / width) * 100)}%`);
    await sleep(18 + Math.random() * 30); // 56k-era jitter
  }
  process.stdout.write("\n");
}

async function update(rows) {
  const todo = rows.filter((r) => r.status === "update");
  if (!todo.length) return console.log("\n " + paint("Everything is up to date. Touch grass.", C.green));
  for (const r of todo) {
    if (r.eco !== "npm") {
      console.log(`\n ${paint("skip", C.gray)} ${r.name} — ${r.eco} updates not wired up yet`);
      continue;
    }
    console.log(`\n ${paint("Downloading", C.cyan)} ${r.name} ${paint("from @" + r.user, C.magenta)} ...`);
    await downloadBar(r.name);
    try {
      sh(`npm i -g ${r.pkg}@latest`);
      console.log("   " + paint(`✓ ${r.name} → ${r.latest}`, C.green));
    } catch {
      console.log("   " + paint(`✗ transfer failed (peer went offline?)`, C.red));
    }
  }
}

// ---------------------------------------------------------------------------
async function splash() {
  console.log(paint("\n Connecting to napm server...", C.dim));
  const steps = [
    "resolving registry.npmjs.org",
    "authenticating as napster_user_2001",
    "joining swarm",
  ];
  for (const s of steps) {
    process.stdout.write(paint(`   > ${s} `, C.dim));
    await sleep(220);
    console.log(paint("ok", C.green));
  }
  console.log(paint("   4,182,007 peers online.  Connected at 56.6 kbps.\n", C.dim));
}

async function main() {
  const args = new Set(process.argv.slice(2));
  const demo = args.has("--demo");
  if (!args.has("--no-splash")) await splash();

  banner();
  console.log();
  const rows = TOOLS.map((t) => resolve(t, demo));
  table(rows);
  statusBar(rows);

  if (args.has("--update")) await update(rows);
}

main().catch((e) => { console.error(e); process.exit(1); });
