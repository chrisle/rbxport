import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { readReleaseNotes } from "./release-notes.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const VERSION = /^(\d+)\.(\d+)\.(\d+)-rc\.(\d+)$/;
const CHANGE = /^\((New|Fixed|Improved)\)\s+\S/;

export function nextVersion(version) {
  const match = VERSION.exec(version);
  if (!match) throw new Error(`Cannot choose a standard next version from ${version}; pass --version X.Y.Z-rc.N`);
  const [, major, minor, patch, rc] = match;
  return `${major}.${minor}.${patch}-rc.${Number(rc) + 1}`;
}

export function parseArgs(args) {
  const options = { dryRun: false, resume: false, notes: [] };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === "--dry-run") options.dryRun = true;
    else if (arg === "--resume") options.resume = true;
    else if (arg === "--version") options.version = args[++i];
    else if (arg === "--note") options.notes.push(args[++i]);
    else throw new Error("Usage: pnpm release:cut -- [--version X.Y.Z-rc.N] --note '(Fixed) …' [--note …] [--dry-run|--resume]");
  }
  if (options.version && !VERSION.test(options.version)) throw new Error("--version must be X.Y.Z-rc.N");
  if (options.notes.some((note) => !CHANGE.test(note))) throw new Error("every --note must start with (New), (Improved), or (Fixed)");
  if (options.dryRun && options.resume) throw new Error("--dry-run and --resume cannot be combined");
  return options;
}

export function candidateAction({ sourceVersion, tagsAtHead, usedTags = tagsAtHead, version, resume }) {
  const candidate = version ?? nextVersion(sourceVersion);
  if (tagsAtHead.includes(`v${sourceVersion}`)) {
    if (resume) return { action: "resume-tag", version: sourceVersion };
    return { action: "prepare", version: candidate };
  }
  if (usedTags.includes(`v${sourceVersion}`)) return { action: "prepare", version: candidate };
  if (resume || version === sourceVersion) return { action: "resume", version: sourceVersion };
  throw new Error(`HEAD is already an untagged ${sourceVersion} candidate; use --resume or --version ${sourceVersion}`);
}

function command(binary, args, { dryRun = false, allowFailure = false } = {}) {
  console.log(`$ ${[binary, ...args].join(" ")}`);
  if (dryRun) return "";
  try {
    return execFileSync(binary, args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch (error) {
    if (allowFailure) return "";
    const detail = `${error.stdout ?? ""}${error.stderr ?? ""}`.trim();
    throw new Error(`${binary} ${args.join(" ")} failed${detail ? `:\n${detail}` : ""}`);
  }
}

function sourceVersion() {
  const cargo = readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8");
  const version = /\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
  if (!version) throw new Error("Cargo.toml has no workspace version");
  return version;
}

function date() { return new Date().toISOString().slice(0, 10); }
function sleep(ms) { return new Promise((resolve) => setTimeout(resolve, ms)); }

function assertReady() {
  if (command("git", ["status", "--porcelain"])) throw new Error("working tree is dirty");
  if (command("git", ["branch", "--show-current"]) !== "dev") throw new Error("run release:cut from checked-out dev");
  command("git", ["fetch", "origin", "dev", "main", "--tags"]);
  if (command("git", ["rev-parse", "HEAD"]) !== command("git", ["rev-parse", "origin/dev"])) {
    throw new Error("local dev is not exactly origin/dev");
  }
  const active = JSON.parse(command("gh", ["run", "list", "--workflow", "Release", "--limit", "30", "--json", "status,databaseId"]) || "[]")
    .filter((run) => ["queued", "in_progress", "waiting", "pending"].includes(run.status));
  if (active.length) throw new Error(`a release is already active: ${active.map((run) => run.databaseId).join(", ")}`);
}

function prepare(version, notes, dryRun) {
  if (!notes.length) throw new Error("a new version needs at least one --note; the command will not invent release notes");
  if (dryRun) return;
  const path = new URL("../release-notes.json", import.meta.url);
  const existing = readReleaseNotes(path);
  if (existing.some((entry) => entry.version === version)) throw new Error(`release-notes.json already contains ${version}`);
  writeFileSync(path, `${JSON.stringify([{ version, date: date(), changes: notes }, ...existing], null, 2)}\n`);
  command("node", ["scripts/sync-version.mjs", "--version", version]);
  command("git", ["add", "Cargo.toml", "Cargo.lock", "package.json", "src-tauri/tauri.conf.json", "release-notes.json"]);
  command("git", ["commit", "-m", `chore(release): prepare ${version}`]);
  command("git", ["push", "origin", "dev"]);
}

async function waitForRun(workflow, sha, event) {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    const runs = JSON.parse(command("gh", ["run", "list", "--workflow", workflow, "--commit", sha, "--event", event, "--limit", "10", "--json", "databaseId,headSha,status"]) || "[]");
    const run = runs.find((item) => item.headSha === sha);
    if (run) {
      command("gh", ["run", "watch", String(run.databaseId), "--exit-status"]);
      return run.databaseId;
    }
    await sleep(2000);
  }
  throw new Error(`no ${workflow} run appeared for ${sha}`);
}

async function verifyPublished(version) {
  const base = command("gh", ["variable", "get", "CDN_BASE_URL"]).replace(/\/$/, "");
  if (!base) throw new Error("CDN_BASE_URL is not configured");
  const response = await fetch(`${base}/latest.json`);
  if (!response.ok) throw new Error(`latest.json returned ${response.status}`);
  const feed = await response.json();
  if (feed.version !== version) throw new Error(`latest.json is ${feed.version}, expected ${version}`);
  for (const item of [...Object.values(feed.platforms ?? {}), ...Object.values(feed.downloads ?? {})]) {
    const result = await fetch(item.url, { method: "HEAD" });
    if (!result.ok) throw new Error(`installer check failed: ${item.url} (${result.status})`);
  }
}

export async function runReleaseCut(options) {
  assertReady();
  const source = sourceVersion();
  const tags = command("git", ["tag", "--points-at", "HEAD", "--list", "v*"]).split("\n").filter(Boolean);
  const usedTags = command("git", ["tag", "--list", `v${source}`]).split("\n").filter(Boolean);
  const candidate = candidateAction({ sourceVersion: source, tagsAtHead: tags, usedTags, version: options.version, resume: options.resume });
  console.log(`${candidate.action} candidate v${candidate.version}`);
  if (options.dryRun) return;
  if (candidate.action === "resume-tag") {
    const sha = command("git", ["rev-parse", "HEAD"]);
    if (command("git", ["rev-parse", "origin/main"]) !== sha) throw new Error("tagged candidate is not exactly origin/main");
    await waitForRun("Release", sha, "push");
    await verifyPublished(candidate.version);
    console.log(`Released ${candidate.version} from ${sha}.`);
    return;
  }
  if (candidate.action === "prepare") prepare(candidate.version, options.notes, false);
  command("pnpm", ["release:preflight", "--", "--expect-untagged"]);
  const sha = command("git", ["rev-parse", "HEAD"]);
  if (candidate.action === "prepare") {
    // Pushing dev already starts the exact-SHA validation workflow.
    await waitForRun("Validate", sha, "push");
  } else {
    // A candidate prepared before this command may not have an observable
    // current validation run, so resume it with an explicit no-publish run.
    command("gh", ["workflow", "run", "Validate", "--ref", "dev", "-f", `source_ref=${sha}`]);
    await waitForRun("Validate", sha, "workflow_dispatch");
  }
  command("git", ["fetch", "origin", "dev", "main", "--tags"]);
  if (command("git", ["rev-parse", "origin/dev"]) !== sha) throw new Error("dev moved during validation");
  command("git", ["merge-base", "--is-ancestor", "origin/main", sha]);
  command("git", ["switch", "main"]);
  command("git", ["merge", "--ff-only", sha]);
  command("git", ["push", "origin", "main"]);
  if (command("git", ["rev-parse", "origin/main"]) !== sha) throw new Error("main did not fast-forward to the validated SHA");
  const tag = `v${candidate.version}`;
  const peeled = command("git", ["ls-remote", "--tags", "origin", `refs/tags/${tag}^{}`]);
  if (peeled && !peeled.startsWith(sha)) throw new Error(`${tag} already exists at another commit`);
  if (!peeled) {
    command("git", ["tag", "-a", tag, "-m", `rbxport ${candidate.version}`, sha]);
    command("git", ["push", "origin", tag]);
  }
  await waitForRun("Release", sha, "push");
  await verifyPublished(candidate.version);
  command("git", ["switch", "dev"]);
  console.log(`Released ${candidate.version} from ${sha}.`);
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  runReleaseCut(parseArgs(process.argv.slice(2))).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
