#!/usr/bin/env node
/**
 * Differential call tracer for S_js fixtures.
 * Usage: node scripts/diff_trace.cjs <module.cjs> <entry>
 * Wraps every exported function, runs entry, prints JSON edges:
 *   [{ "from": "main", "to": "loginHandler" }, ...]
 */
"use strict";
const path = require("path");

const modPath = process.argv[2];
const entry = process.argv[3] || "main";
if (!modPath) {
  console.error("usage: diff_trace.cjs <module> [entry]");
  process.exit(2);
}

const abs = path.resolve(modPath);
const app = require(abs);

const edges = [];
const names = Object.keys(app).filter((k) => typeof app[k] === "function");
let current = null;

for (const name of names) {
  const orig = app[name];
  app[name] = function wrapped(...args) {
    const prev = current;
    if (prev) {
      edges.push({ from: prev, to: name });
    }
    current = name;
    try {
      return orig.apply(this, args);
    } finally {
      current = prev;
    }
  };
}

if (typeof app[entry] !== "function") {
  console.error("entry not found:", entry);
  process.exit(2);
}

current = null;
app[entry]();

// unique
const seen = new Set();
const uniq = [];
for (const e of edges) {
  const k = e.from + "→" + e.to;
  if (!seen.has(k)) {
    seen.add(k);
    uniq.push(e);
  }
}
process.stdout.write(JSON.stringify({ entry, edges: uniq }, null, 0));
