#!/usr/bin/env node

/**
 * Removes regenerable checkout output via pnpm clean.
 * Use pnpm clean -- --dry-run to preview. --app-data additionally removes caches
 * and abandoned partial backups; --dependencies removes node_modules; --git
 * prunes stale worktree records and eligible merged local branches.
 * Preserves completed backups, preferences, logs, recovery data, and libraries.
 * --app-data requires the desktop app to be closed.
 */
import { lstat, opendir, readFile, rm } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY = path.resolve(SCRIPT_DIR, "..");
const PROJECT_OUTPUT = [
  "target",
  "dist",
  "test-results",
  "playwright-report",
  "dist-mock",
];
const PARTIAL_BACKUP = /^\.partial-[0-9a-f-]+(?:\.zip)?$/i;

function usage() {
  return `Usage: npm run clean -- [options]

Removes generated RBXport files. With no options, only regenerable output in
this checkout is removed.

Options:
  --app-data       Also remove RBXport caches and abandoned backup work
  --dependencies   Also remove node_modules (run pnpm install to restore it)
  --git             Also prune stale worktree records and merged local branches
  --dry-run        Show what would be removed without changing anything
  --help           Show this help

Completed library backups, preferences, logs, recovery journals, and rekordbox
libraries are never removed. Close RBXport before using --app-data.`;
}

export function parseArguments(args) {
  const options = { appData: false, dependencies: false, git: false, dryRun: false, help: false };
  for (const argument of args) {
    if (argument === "--") continue;
    if (argument === "--app-data") options.appData = true;
    else if (argument === "--dependencies") options.dependencies = true;
    else if (argument === "--git") options.git = true;
    else if (argument === "--dry-run") options.dryRun = true;
    else if (argument === "--help" || argument === "-h") options.help = true;
    else throw new Error(`Unknown option: ${argument}\n\n${usage()}`);
  }
  return options;
}

function appDirectories(platform = process.platform, environment = process.env) {
  const home = homedir();
  if (platform === "darwin") {
    return {
      caches: [
        path.join(home, "Library/Caches/com.rbxport.triode"),
        path.join(home, "Library/Caches/rbxport-triode"),
      ],
      state: path.join(home, "Library/Application Support/rbxport-triode/backups"),
    };
  }
  if (platform === "win32") {
    const local = environment.LOCALAPPDATA;
    const roaming = environment.APPDATA;
    return {
      caches: local ? [path.join(local, "com.rbxport.triode"), path.join(local, "rbxport-triode")] : [],
      state: roaming ? path.join(roaming, "rbxport-triode/backups") : path.join(tmpdir(), "rbxport-triode/backups"),
    };
  }
  const cache = environment.XDG_CACHE_HOME || path.join(home, ".cache");
  const data = environment.XDG_DATA_HOME || path.join(home, ".local/share");
  return {
    caches: [path.join(cache, "com.rbxport.triode"), path.join(cache, "rbxport-triode")],
    state: path.join(data, "rbxport-triode/backups"),
  };
}

async function existing(pathname) {
  try {
    return await lstat(pathname);
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
}

async function allocatedBytes(pathname, seen = new Set()) {
  const metadata = await existing(pathname);
  if (!metadata) return 0;
  const identity = `${metadata.dev}:${metadata.ino}`;
  if (seen.has(identity)) return 0;
  seen.add(identity);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    return metadata.blocks ? metadata.blocks * 512 : metadata.size;
  }
  let bytes = metadata.blocks ? metadata.blocks * 512 : metadata.size;
  const directory = await opendir(pathname);
  for await (const entry of directory) {
    bytes += await allocatedBytes(path.join(pathname, entry.name), seen);
  }
  return bytes;
}

async function configuredBackupDestination(stateDirectory) {
  try {
    const configured = JSON.parse(await readFile(path.join(stateDirectory, "backup-destination.json"), "utf8"));
    return typeof configured === "string" && path.isAbsolute(configured) ? configured : stateDirectory;
  } catch (error) {
    if (error?.code === "ENOENT" || error instanceof SyntaxError) return stateDirectory;
    throw error;
  }
}

async function abandonedBackups(directory) {
  const metadata = await existing(directory);
  if (!metadata?.isDirectory() || metadata.isSymbolicLink()) return [];
  const candidates = [];
  const entries = await opendir(directory);
  for await (const entry of entries) {
    if (PARTIAL_BACKUP.test(entry.name)) candidates.push(path.join(directory, entry.name));
  }
  return candidates;
}

export async function cleanupPlan(options, repository = REPOSITORY, app = appDirectories()) {
  const targets = PROJECT_OUTPUT.map(output => ({ kind: "generated output", path: path.join(repository, output) }));
  if (options.dependencies) targets.push({ kind: "dependencies", path: path.join(repository, "node_modules") });
  if (options.appData) {
    for (const cache of app.caches) targets.push({ kind: "app cache", path: cache });
    const destination = await configuredBackupDestination(app.state);
    const roots = [...new Set([app.state, destination])];
    for (const root of roots) {
      for (const partial of await abandonedBackups(root)) {
        targets.push({ kind: "abandoned backup work", path: partial });
      }
    }
  }
  return targets;
}

function appIsRunning(platform = process.platform) {
  if (platform === "win32") {
    const result = spawnSync("tasklist", ["/FI", "IMAGENAME eq rbxport.exe", "/FO", "CSV", "/NH"], { encoding: "utf8" });
    return result.status === 0 && /"rbxport\.exe"/i.test(result.stdout);
  }
  const result = spawnSync("pgrep", ["-x", "rbxport"], { encoding: "utf8" });
  return result.status === 0 && result.stdout.trim().length > 0;
}

function git(repository, args, includeStderr = false) {
  const result = spawnSync("git", args, { cwd: repository, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `git ${args.join(" ")} failed`);
  }
  return result.stdout + (includeStderr ? result.stderr : "");
}

export function mergedBranchCandidates(merged, checkedOut, current) {
  const protectedBranches = new Set(["main", "master", "dev", "trunk", current, ...checkedOut]);
  return merged.filter(branch => branch && !protectedBranches.has(branch));
}

function lines(output) {
  return output.split("\n").map(line => line.trim()).filter(Boolean);
}

function gitCleanupPlan(repository) {
  const worktrees = git(repository, ["worktree", "list", "--porcelain"]);
  const checkedOut = lines(worktrees)
    .filter(line => line.startsWith("branch refs/heads/"))
    .map(line => line.slice("branch refs/heads/".length));
  const current = git(repository, ["branch", "--show-current"]).trim();
  const merged = lines(git(repository, ["branch", "--merged", "HEAD", "--format=%(refname:short)"]));
  const branches = mergedBranchCandidates(merged, checkedOut, current);
  const staleWorktrees = lines(git(repository, ["worktree", "prune", "--dry-run", "--verbose"], true));
  return { branches, staleWorktrees };
}

function cleanGit(repository, dryRun) {
  const plan = gitCleanupPlan(repository);
  for (const branch of plan.branches) {
    console.log(`${dryRun ? "Would delete" : "Deleting"} merged local branch: ${branch}`);
    if (!dryRun) git(repository, ["branch", "--delete", "--", branch]);
  }
  for (const worktree of plan.staleWorktrees) {
    console.log(`${dryRun ? "Would prune" : "Pruning"} stale worktree record: ${worktree}`);
  }
  if (!dryRun && plan.staleWorktrees.length > 0) git(repository, ["worktree", "prune", "--verbose"]);
  if (plan.branches.length === 0 && plan.staleWorktrees.length === 0) {
    console.log("No merged local branches or stale worktree records to clean.");
  }
}

function sizeLabel(bytes) {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / (1024 ** exponent);
  return `${value >= 10 || exponent === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[exponent]}`;
}

export async function run(args = process.argv.slice(2)) {
  const options = parseArguments(args);
  if (options.help) {
    console.log(usage());
    return;
  }
  if (options.appData && !options.dryRun && appIsRunning()) {
    throw new Error("RBXport is running. Close it before cleaning app data.");
  }

  const plan = await cleanupPlan(options);
  let reclaimed = 0;
  let found = 0;
  for (const target of plan) {
    const metadata = await existing(target.path);
    if (!metadata) continue;
    const bytes = await allocatedBytes(target.path);
    reclaimed += bytes;
    found += 1;
    console.log(`${options.dryRun ? "Would remove" : "Removing"} ${target.kind}: ${target.path} (${sizeLabel(bytes)})`);
    if (!options.dryRun) await rm(target.path, { recursive: true, force: true });
  }
  if (found === 0) {
    console.log("Nothing to clean.");
  } else {
    console.log(`${options.dryRun ? "Reclaimable" : "Reclaimed"}: ${sizeLabel(reclaimed)}`);
  }
  if (options.git) cleanGit(REPOSITORY, options.dryRun);
}

const invoked = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invoked) {
  run().catch(error => {
    console.error(`cleanup: ${error.message}`);
    process.exitCode = 1;
  });
}
