/**
 * Synchronizes versions in package.json, Tauri config, Cargo.toml, and
 * workspace package entries in Cargo.lock, leaving third-party versions alone.
 * Run: node scripts/sync-version.mjs [--version VERSION]. Without an explicit
 * version, uses the newest valid release tag merged into HEAD. Writes changed
 * files; desktop dev and release/installer workflows invoke it.
 */
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
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

// Only RBXport's application and crates inherit the workspace version. A
// local dependency such as the vendored updater also has no lockfile source.
const crates = new URL("../crates/", import.meta.url);
const crateManifests = readdirSync(crates, { withFileTypes: true })
  .filter(entry => entry.isDirectory())
  .map(entry => new URL(`${entry.name}/Cargo.toml`, crates));
const appManifest = new URL("../src-tauri/Cargo.toml", import.meta.url);
const workspacePackages = new Set();
for (const manifest of [appManifest, ...crateManifests]) {
  const packageTable = readFileSync(manifest, "utf8")
    .split(/(?=^\[)/m)
    .find(table => /^\[package\]\s*$/m.test(table)) ?? "";
  if (!/^version\.workspace\s*=\s*true\s*(?:#.*)?$/m.test(packageTable)) continue;
  const name = packageTable.match(/^name\s*=\s*"([^"]+)"\s*(?:#.*)?$/m)?.[1];
  if (!name || workspacePackages.has(name)) throw new Error(`Invalid workspace package name in ${fileURLToPath(manifest)}`);
  workspacePackages.add(name);
}

const cargoManifest = new URL("../Cargo.toml", import.meta.url);
const workspaceVersion = /^(\[workspace\.package\][\s\S]*?^version\s*=\s*)"([^"]+)"/m;
const currentVersion = readFileSync(cargoManifest, "utf8").match(workspaceVersion)?.[2];
if (!currentVersion) throw new Error("Missing workspace package version");

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
update("Cargo.toml", text => text.replace(workspaceVersion, `$1"${version}"`));
update("Cargo.lock", text => text.split(/(?=^\[\[package\]\])/m).map(block => {
  const name = block.match(/^name = "([^"]+)"/m)?.[1];
  if (!block.startsWith("[[package]]") || !workspacePackages.has(name) || /^source\s*=/m.test(block)) return block;
  return block.replace(/^version = "([^"]+)"/m, (line, lockedVersion) =>
    lockedVersion === currentVersion ? `version = "${version}"` : line);
}).join(""));
for (const [path, text] of updates) writeFileSync(path, text);
console.log(`Version ${version}${updates.size ? ` (${updates.size} files updated)` : " (unchanged)"}`);
