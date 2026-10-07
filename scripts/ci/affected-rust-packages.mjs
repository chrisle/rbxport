import { readFileSync } from "node:fs";
import path, { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export function affectedRustPackages(metadata, changedPaths) {
  const workspaceIds = new Set(metadata.workspace_members);
  const packages = metadata.packages.filter(pkg => workspaceIds.has(pkg.id));
  const workspaceRoot = metadata.workspace_root;
  const byDirectory = new Map(packages.map(pkg => [
    path.relative(workspaceRoot, path.dirname(pkg.manifest_path)).replaceAll(path.sep, "/"),
    pkg,
  ]));

  const changed = new Set();
  for (const rawPath of changedPaths) {
    const changedPath = rawPath.replace(/^\.\//, "").trim();
    if (!changedPath) continue;

    // Workspace-wide dependency/toolchain or application-shell changes can
    // affect every package. Do not attempt to narrow those.
    if (/^(Cargo\.toml|Cargo\.lock|rust-toolchain(?:\.toml)?|\.cargo\/|iTunes\/)/.test(changedPath)) {
      return null;
    }

    const owner = [...byDirectory.entries()]
      .sort(([a], [b]) => b.length - a.length)
      .find(([directory]) => changedPath === directory || changedPath.startsWith(`${directory}/`));
    if (owner) changed.add(owner[1].name);
    else if (changedPath.endsWith(".rs") || changedPath.endsWith(".toml")) return null;
  }

  if (changed.size === 0) return null;

  const reverseDependencies = new Map(packages.map(pkg => [pkg.name, new Set()]));
  for (const pkg of packages) {
    for (const dependency of pkg.dependencies) {
      if (reverseDependencies.has(dependency.name)) {
        reverseDependencies.get(dependency.name).add(pkg.name);
      }
    }
  }

  const queue = [...changed];
  while (queue.length > 0) {
    const dependency = queue.shift();
    for (const dependent of reverseDependencies.get(dependency) ?? []) {
      if (changed.has(dependent)) continue;
      changed.add(dependent);
      queue.push(dependent);
    }
  }

  return [...changed].sort();
}

function main() {
  const metadataPath = process.argv[2];
  if (!metadataPath) throw new Error("usage: affected-rust-packages.mjs <cargo-metadata.json>");
  const metadata = JSON.parse(readFileSync(metadataPath, "utf8"));
  const changedPaths = readFileSync(0, "utf8").split(/\0|\r?\n/);
  const packages = affectedRustPackages(metadata, changedPaths);
  process.stdout.write(packages ? `${packages.join("\n")}\n` : "--workspace\n");
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
