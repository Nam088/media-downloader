#!/usr/bin/env node
// Release bot core: decides the next version from the conventional commits
// since the last release tag, then writes the CHANGELOG.md entry for it.
//
// Usage: node scripts/auto-release.mjs <date>
//   <date> is YYYY-MM-DD, passed in because CI gives the caller full control
//   over the timestamp source.
//
// stdout contract (parsed by the prepare job in .github/workflows/release.yml):
//   skip=true          no feat/fix/breaking commits since the last release
//   version=x.y.z      the next version (only when not skipping)
//   type=<bump>        major | minor | patch
//
// Side effects: prepends the new version's section to CHANGELOG.md and writes
// the same section (minus the heading) to RELEASE_NOTES.md, which the release
// job publishes as the GitHub release body.
//
// Bump rules, same as semantic-release:
//   BREAKING CHANGE footer or `type!:` subject  -> major
//   feat:                                       -> minor
//   fix: / perf:                                -> patch
//   anything else (docs, chore, ci, refactor..) -> no release at all

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const sh = (args) => execFileSync("git", args, { encoding: "utf8" }).trim();

const date = process.argv[2] || new Date().toISOString().slice(0, 10);

const lastTag =
  sh(["tag", "--list", "v*", "--sort", "-v:refname"]).split("\n").filter(Boolean)[0] ||
  null;

const range = lastTag ? `${lastTag}..HEAD` : "HEAD";
// \x00 separates fields, \x01 separates records — commit bodies contain
// newlines, so line-based parsing is not an option.
const raw = sh(["log", range, "--pretty=format:%H%x00%s%x00%b%x01"]);

const CONV = /^(\w+)(\([^)]*\))?(!)?:\s*(.+)/;
const breaking = [];
const features = [];
const fixes = [];
const perfs = [];

for (const record of raw.split("\x01").filter(Boolean)) {
  const [hash, subject, body] = record.replace(/^\n/, "").split("\x00");
  const commit = { hash, subject };
  if (/BREAKING[ -]CHANGE/.test(body || "") || CONV.exec(subject)?.[3] === "!") {
    breaking.push(commit);
    continue;
  }
  const match = CONV.exec(subject);
  if (!match) continue;
  const type = match[1].toLowerCase();
  if (type === "feat" || type === "feature") features.push(commit);
  else if (type === "fix") fixes.push(commit);
  else if (type === "perf") perfs.push(commit);
}

if (!breaking.length && !features.length && !fixes.length && !perfs.length) {
  console.log("skip=true");
  process.exit(0);
}

const current = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const [major, minor, patch] = current.split(".").map(Number);
let bump;
if (breaking.length) bump = "major";
else if (features.length) bump = "minor";
else bump = "patch";
const next =
  bump === "major"
    ? `${major + 1}.0.0`
    : bump === "minor"
      ? `${major}.${minor + 1}.0`
      : `${major}.${minor}.${patch + 1}`;

const lines = [`## ${next} (${date})`, ""];
for (const [title, items] of [
  ["Breaking Changes", breaking],
  ["Features", features],
  ["Bug Fixes", fixes],
  ["Performance Improvements", perfs],
]) {
  if (!items.length) continue;
  lines.push(`### ${title}`, "");
  for (const c of items) lines.push(`- ${c.subject} (${c.hash.slice(0, 7)})`);
  lines.push("");
}
const entry = lines.join("\n");

const HEADER =
  "# Changelog\n\n" +
  "All notable changes to this project are documented here. Generated\n" +
  "automatically from conventional commits by scripts/auto-release.mjs.\n\n";
const old = existsSync("CHANGELOG.md") ? readFileSync("CHANGELOG.md", "utf8") : HEADER;
const anchor = old.indexOf("\n## ");
const changelog = anchor === -1 ? `${old}\n${entry}` : old.slice(0, anchor + 1) + entry + old.slice(anchor + 1);
writeFileSync("CHANGELOG.md", changelog);

writeFileSync("RELEASE_NOTES.md", entry.replace(/^## .*\n\n?/, ""));

console.log(`version=${next}`);
console.log(`type=${bump}`);
