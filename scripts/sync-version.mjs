/**
 * Synchronizes versions in package.json, Tauri config, Cargo.toml, and
 * workspace package entries in Cargo.lock, leaving third-party versions alone.
 * Run: node scripts/sync-version.mjs [--version VERSION]. Without an explicit
 * version, uses the newest valid release tag merged into HEAD. Writes changed
 * files; desktop dev and release/installer workflows invoke it.
 */
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const args = process.argv.slice(2);
if (args.length && (args.length !== 2 || args[0] !== "--version")) {
  throw new Error("Usage: node scripts/sync-version.mjs [--version VERSION]");
}
// Only tags included in this checkout: newer releases on other branches
// must not make older source advertise their version.
const version = args.length ? args[1] : execFileSync("git", [
  "tag", "--merged", "HEAD", "--sort=-version:refname",
], { cwd: root, encoding: "utf8" }).split("\n")
  .find(tag => /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(tag))?.slice(1);
if (!version || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(version)) {
  throw new Error("No valid release version. Fetch release tags or pass --version VERSION.");
}

const updates = new Map();
function update(file, transform) {
  const path = new URL(`../${file}`, import.meta.url);
  const before = readFileSync(path, "utf8");
  const after = transform(before);
  if (before !== after) updates.set(path, after);
}
for (const file of ["package.json", "src-tauri/tauri.conf.json"]) {
  update(file, text => {
    const json = JSON.parse(text);
    json.version = version;
    return JSON.stringify(json, null, 2) + "\n";
  });
}
update("Cargo.toml", text => {
  const pattern = /(\[workspace\.package\][\s\S]*?^version\s*=\s*)"[^"]+"/m;
  if (!pattern.test(text)) throw new Error("Missing workspace package version");
  return text.replace(pattern, `$1"${version}"`);
});
// A path dependency has no source field either. Read the workspace members'
// package names so vendored crates keep their upstream version.
const crateDirectories = readdirSync(new URL("../crates/", import.meta.url), { withFileTypes: true })
  .filter(entry => entry.isDirectory())
  .map(entry => `crates/${entry.name}`);
const workspacePackages = new Set();
for (const directory of ["src-tauri", ...crateDirectories]) {
  const manifest = readFileSync(new URL(`../${directory}/Cargo.toml`, import.meta.url), "utf8");
  const packageSection = manifest.split(/^\[/m).find(section => section.startsWith("package]"));
  const name = packageSection?.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
  if (name && /^version\.workspace\s*=\s*true\s*$/m.test(packageSection)) {
    workspacePackages.add(name);
  }
}
update("Cargo.lock", text => text.split(/(?=^\[\[package\]\])/m).map(block => {
  const name = block.match(/^name = "([^"]+)"/m)?.[1];
  if (!workspacePackages.has(name) || /^source\s*=/m.test(block)) return block;
  return block.replace(/^version = "[^"]+"/m, `version = "${version}"`);
}).join(""));
for (const [path, text] of updates) writeFileSync(path, text);
console.log(`Version ${version}${updates.size ? ` (${updates.size} files updated)` : " (unchanged)"}`);
