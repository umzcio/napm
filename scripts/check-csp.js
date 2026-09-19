#!/usr/bin/env node
/*
 * CSP parity checker. Zero dependencies, plain Node.
 *
 * WHY THIS EXISTS
 *
 * `npm run tauri dev` does NOT enforce the `csp` from tauri.conf.json, and in
 * packaged builds Tauri appends a nonce to `style-src`, which per CSP Level 3
 * voids `'unsafe-inline'`. Two P1 defects shipped through that gap:
 *
 *   - plan 034: the CSP blocked the app's own font fetch (connect-src hole).
 *   - plan 035: inline style attributes were silently dropped, leaving the
 *     released app dimmed and unclickable.
 *
 * Neither defect reproduced in dev. This script statically asserts the exact
 * invariant classes behind both defects, so the next CSP-adjacent change
 * fails CI before release instead of after. Do not delete it as noise; if it
 * blocks a legitimate change, extend the checks, do not weaken them.
 *
 * Run: `node scripts/check-csp.js` or `npm run check:csp`. Exits 0 when every
 * check passes, 1 with one line per failure otherwise.
 */

"use strict";

const fs = require("fs");
const path = require("path");

const ROOT = path.resolve(__dirname, "..");
const CONF_PATH = path.join(ROOT, "src-tauri", "tauri.conf.json");
const HTML_PATH = path.join(ROOT, "frontend", "index.html");

const failures = [];
const passes = [];

function fail(msg) {
  failures.push(msg);
}
function pass(msg) {
  passes.push(msg);
}

// --- Load and parse the production CSP -------------------------------------

let conf;
try {
  conf = JSON.parse(fs.readFileSync(CONF_PATH, "utf8"));
} catch (e) {
  console.error(`FAIL: cannot read or parse ${CONF_PATH}: ${e.message}`);
  process.exit(1);
}

const csp = conf && conf.app && conf.app.security && conf.app.security.csp;
if (typeof csp !== "string" || csp.trim() === "") {
  console.error(
    "FAIL: app.security.csp is missing from src-tauri/tauri.conf.json. " +
      "The CSP shape this checker guards has changed; update the checker in the same change."
  );
  process.exit(1);
}

// directive name -> Set of sources
const directives = new Map();
for (const part of csp.split(";")) {
  const tokens = part.trim().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) continue;
  directives.set(tokens[0], new Set(tokens.slice(1)));
}

// Emulate the packaged build: Tauri appends a nonce to style-src at runtime,
// and per CSP Level 3 a nonce makes 'unsafe-inline' be ignored. That is the
// condition that killed every inline style attribute in v0.1.5/v0.1.6. (Plan
// 035 evidence: only styles broke, so script-src nonces are not emulated.)
const styleSrc = new Set(directives.get("style-src") || []);
styleSrc.add("'nonce-x'");
directives.set("style-src", styleSrc);

// --- Load the frontend markup -----------------------------------------------

let html;
try {
  html = fs.readFileSync(HTML_PATH, "utf8");
} catch (e) {
  console.error(`FAIL: cannot read ${HTML_PATH}: ${e.message}`);
  process.exit(1);
}

// Strip /* ... */ comment blocks before scanning: the head stylesheet's own
// comment documents the guard (and contains a literal style=" example), so
// raw scanning would flag the documentation itself. Comments are replaced by
// their own newlines (not deleted outright) so reported line numbers still
// match the real file.
const stripped = html.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ""));
const lines = stripped.split("\n");

// --- Check 1: CSP parse sanity ----------------------------------------------

const REQUIRED_DIRECTIVES = ["default-src", "script-src", "style-src", "font-src", "connect-src"];
{
  const missing = REQUIRED_DIRECTIVES.filter((d) => !directives.has(d));
  if (missing.length > 0) {
    fail(`CSP is missing required directives: ${missing.join(", ")}`);
  } else {
    pass(`CSP parse sanity: ${REQUIRED_DIRECTIVES.join(", ")} all present`);
  }
}

// --- Check 2: no inline style attributes -------------------------------------
//
// With the packaged-build nonce in style-src, 'unsafe-inline' is void, so any
// style="..." attribute is dead in production (plan 035).
{
  const re = /style\s*=\s*["']/;
  const hits = [];
  lines.forEach((line, i) => {
    if (re.test(line)) hits.push(i + 1);
  });
  if (hits.length > 0) {
    fail(
      `inline style attribute(s) in frontend/index.html at line(s) ${hits.join(", ")}. ` +
        "Tauri's style-src nonce voids 'unsafe-inline' in packaged builds; " +
        "move the styling into the head stylesheet or set it from JS via the CSSOM."
    );
  } else {
    pass("no inline style attributes in frontend/index.html");
  }
}

// --- Check 3: exactly one <style tag -----------------------------------------
//
// The head stylesheet is the single tag Tauri rewrites with its nonce. A
// second <style> tag created in markup would not carry the nonce and would be
// dead in production.
{
  const count = (html.match(/<style/g) || []).length;
  if (count !== 1) {
    fail(
      `expected exactly one <style tag in frontend/index.html, found ${count}. ` +
        "Only the head stylesheet receives Tauri's nonce; additional <style> tags are blocked in packaged builds."
    );
  } else {
    pass("exactly one <style tag (the head stylesheet Tauri nonces)");
  }
}

// --- Check 4: no inline event-handler attributes -----------------------------
{
  const re = /\son[a-z]+\s*=\s*["']/;
  const hits = [];
  lines.forEach((line, i) => {
    if (re.test(line)) hits.push(i + 1);
  });
  if (hits.length > 0) {
    fail(
      `inline event-handler attribute(s) in frontend/index.html at line(s) ${hits.join(", ")}. ` +
        "Wire events with addEventListener instead."
    );
  } else {
    pass("no inline event-handler attributes in frontend/index.html");
  }
}

// --- Check 5: no javascript: URLs --------------------------------------------
{
  const re = /javascript:/i;
  const hits = [];
  lines.forEach((line, i) => {
    if (re.test(line)) hits.push(i + 1);
  });
  if (hits.length > 0) {
    fail(`javascript: URL(s) in frontend/index.html at line(s) ${hits.join(", ")}`);
  } else {
    pass("no javascript: URLs in frontend/index.html");
  }
}

// --- Check 6: font fetch coverage (plan 034 regression class) ----------------
//
// The app loads vt323.ttf twice: via CSS @font-face (governed by font-src)
// and via a JS fetch()/FontFace fallback (governed by connect-src). If anyone
// removes 'self' from connect-src, the packaged app loses its font again.
{
  const hasFontFace = /@font-face/.test(html) && /vt323\.ttf/.test(html);
  const hasJsLoad = /fetch\(["']\/vt323\.ttf/.test(html) || /FontFace/.test(html);
  if (!hasFontFace) {
    fail("frontend/index.html no longer references vt323.ttf via @font-face; update this check with the new font wiring");
  } else if (!hasJsLoad) {
    fail("frontend/index.html no longer loads vt323.ttf from JS (fetch/FontFace); update this check with the new font wiring");
  } else {
    const fontSrc = directives.get("font-src") || new Set();
    const connectSrc = directives.get("connect-src") || new Set();
    if (!fontSrc.has("'self'")) {
      fail("font-src lacks 'self': the CSS @font-face load of vt323.ttf would be blocked in packaged builds (plan 034 class)");
    } else if (!connectSrc.has("'self'")) {
      fail("connect-src lacks 'self': the JS fetch of /vt323.ttf would be blocked in packaged builds (plan 034 class)");
    } else {
      pass("font fetch covered: font-src and connect-src both include 'self'");
    }
  }
}

// --- Report -------------------------------------------------------------------

for (const msg of passes) console.log(`ok: ${msg}`);
if (failures.length > 0) {
  for (const msg of failures) console.error(`FAIL: ${msg}`);
  process.exit(1);
}
console.log("all CSP parity checks passed");
