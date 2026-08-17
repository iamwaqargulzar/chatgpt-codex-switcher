#!/usr/bin/env node
// CodexDesk version helper — keeps the version in sync across
// package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json.
//
// Usage: node scripts/bump-version.mjs [patch|minor|major|0.1.2]
// Defaults to a patch bump.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const targets = [
  { path: "package.json", re: /("version"\s*:\s*)"([^"]+)"/ },
  { path: "src-tauri/Cargo.toml", re: /(^version\s*=\s*)"([^"]+)"/m },
  { path: "src-tauri/tauri.conf.json", re: /("version"\s*:\s*)"([^"]+)"/ },
];

function readVersion(file) {
  const text = readFileSync(resolve(root, file), "utf8");
  const t = targets.find((x) => x.path === file);
  const m = text.match(t.re);
  if (!m || !/^\d+\.\d+\.\d+/.test(m[2])) {
    throw new Error(`${file}: no version line found`);
  }
  return { text, raw: m[2] };
}

const pkg = readVersion("package.json");
const cargo = readVersion("src-tauri/Cargo.toml");
const conf = readVersion("src-tauri/tauri.conf.json");

if (new Set([pkg.raw, cargo.raw, conf.raw]).size !== 1) {
  throw new Error(
    `versions are out of sync: package.json=${pkg.raw} Cargo.toml=${cargo.raw} tauri.conf.json=${conf.raw}`,
  );
}

const current = pkg.raw;
const arg = process.argv[2] ?? "patch";

let next;
if (/^\d+\.\d+\.\d+$/.test(arg)) {
  next = arg;
} else {
  const [maj, min, pat] = current.split(".").map(Number);
  if (arg === "major") next = `${maj + 1}.0.0`;
  else if (arg === "minor") next = `${maj}.${min + 1}.0`;
  else if (arg === "patch") next = `${maj}.${min}.${pat + 1}`;
  else throw new Error(`unknown bump: ${arg}`);
}

if (next === current) {
  console.log(`already at ${current}`);
  process.exit(0);
}

for (const t of targets) {
  const { text } = readVersion(t.path);
  const updated = text.replace(t.re, (_m, prefix) => `${prefix}"${next}"`);
  writeFileSync(resolve(root, t.path), updated);
}

console.log(`${current} -> ${next}`);
console.log("run `cargo check` in src-tauri once to sync Cargo.lock");
