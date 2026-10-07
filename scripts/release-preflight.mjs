/**
 * Checks version agreement across Cargo.toml, package.json, and Tauri config,
 * plus the matching first release-note entry and optional tag.
 * Run: node scripts/release-preflight.mjs [--version VERSION]
 * [--require-tag | --expect-untagged]. Read-only; Release CI fails on mismatch.
 */
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { readReleaseNotes, releaseForVersion } from "./release-notes.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const VERSION = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/;

function usage() {
  throw new Error("Usage: release-preflight.mjs [--version VERSION] [--require-tag | --expect-untagged]");
}

function sourceVersion() {
  const cargo = readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8");
  const match = /\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m.exec(cargo);
  if (!match || !VERSION.test(match[1])) throw new Error("Cargo.toml has no valid workspace version");
  return match[1];
}

function jsonVersion(file) {
  const value = JSON.parse(readFileSync(new URL(`../${file}`, import.meta.url), "utf8")).version;
  if (typeof value !== "string" || !VERSION.test(value)) throw new Error(`${file} has no valid version`);
  return value;
}

function validateWindowsUpdater() {
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const nsis = config.bundle?.windows?.nsis;
  if (nsis?.installMode !== "perMachine") {
    throw new Error("the Windows installer must remain perMachine for the protected update task");
  }
  if (nsis?.installerHooks !== "windows/hooks.nsh") {
    throw new Error("the Windows installer must install the protected update task");
  }
  if (config.bundle?.resources?.["windows/install-update-task.ps1"] !== "install-update-task.ps1") {
    throw new Error("the Windows installer must bundle the protected update task script");
  }
  if (config.plugins?.updater?.windows?.installMode !== "quiet") {
    throw new Error("Windows updates must use the non-interactive installer mode");
  }
}

function tagsAtHead() {
  return execFileSync("git", ["tag", "--points-at", "HEAD", "--list", "v*"], {
    cwd: root,
    encoding: "utf8",
  }).split("\n").filter(Boolean);
}

function main(args) {
  let version;
  let requireTag = false;
  let expectUntagged = false;
  for (let index = 0; index < args.length; index += 1) {
    switch (args[index]) {
      case "--version":
        version = args[++index];
        if (!version || !VERSION.test(version)) usage();
        break;
      case "--require-tag": requireTag = true; break;
      case "--expect-untagged": expectUntagged = true; break;
      default: usage();
    }
  }
  if (requireTag && expectUntagged) usage();

  const expected = version ?? sourceVersion();
  for (const [file, actual] of [
    ["Cargo.toml", sourceVersion()],
    ["package.json", jsonVersion("package.json")],
    ["src-tauri/tauri.conf.json", jsonVersion("src-tauri/tauri.conf.json")],
  ]) {
    if (actual !== expected) throw new Error(`${file} is ${actual}; expected ${expected}`);
  }
  validateWindowsUpdater();
  const notes = readReleaseNotes(new URL("../release-notes.json", import.meta.url));
  releaseForVersion(notes, expected);
  if (notes[0].version !== expected) {
    throw new Error(`release ${expected} must be the first entry in release-notes.json`);
  }

  const releaseTag = `v${expected}`;
  const tags = tagsAtHead();
  if (requireTag && !tags.includes(releaseTag)) {
    throw new Error(`HEAD must be tagged ${releaseTag}; found ${tags.join(", ") || "no release tag"}`);
  }
  if (expectUntagged && tags.some((tag) => /^v/.test(tag))) {
    throw new Error(`HEAD already has a release tag: ${tags.join(", ")}`);
  }
  console.log(`Release preflight passed for ${expected}${requireTag ? ` (${releaseTag})` : ""}.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2));
}
