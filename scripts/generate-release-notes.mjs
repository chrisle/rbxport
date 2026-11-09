/**
 * Prepends a release entry to release-notes.json from Git commits between
 * PREVIOUS and SOURCE_REF. Uses Release-Note trailers or eligible product-change
 * subjects, excluding maintenance and duplicate notes; refuses an empty release.
 * Run: VERSION=X.Y.Z PREVIOUS=TAG SOURCE_REF=REF node scripts/generate-release-notes.mjs.
 * PREVIOUS may be omitted for full history. Used by the Release workflow.
 */
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { groupChanges, validateContributorCredits, validateCuratedChanges } from "./curate-release-notes.mjs";

// Release-Note trailers are the authoritative reader-facing description.
// Without one, include only product changes; never publish CI/test maintenance.
export function changesFromCommits(commits) {
  const changes = [];
  for (const { subject, body = "", files = [] } of commits) {
    const explicit = [...body.matchAll(/^Release-Note:\s*(\((?:New|Improved|Fixed)\)\s+.+)$/gm)];
    if (explicit.length) {
      changes.push(...explicit.map(match => match[1]));
      continue;
    }
    const match = /^(feat|fix|perf)(?:\(([^)]+)\))?:\s*(.+)$/i.exec(subject);
    if (!match || /^(ci|build|test|release)$/i.test(match[2] ?? "")) continue;
    const text = match[3];
    if (/\b(clippy|MSRV|assertions?|tests?|release|validation|xauth|workflow|preflight)\b/i.test(text)) continue;
    if (!files.some(file => /^(src\/|src-tauri\/src\/|crates\/[^/]+\/src\/)/.test(file))) continue;
    // A ticket in a commit body may be incidental. Only the explicit scope or
    // a reviewed Release-Note trailer associates a bug number with the change.
    const ticket = /^RBX-\d+$/i.test(match[2] ?? "") ? match[2].toUpperCase() : "";
    const type = match[1].toLowerCase();
    const label = ticket || type === "fix" ? "Fixed" : type === "feat" ? "New" : "Improved";
    changes.push(`(${label}) ${ticket ? `${ticket}: ` : ""}${text.charAt(0).toUpperCase()}${text.slice(1).replace(/\.$/, "")}.`);
  }
  return groupChanges([...new Set(changes)]);
}

export function generateReleaseNotes(version, previous, source, curatedJson = process.env.RELEASE_NOTES_JSON) {
  const range = previous ? `${previous}..${source}` : source;
  const git = args => execFileSync("git", args, { encoding: "utf8" });
  const commits = git(["log", "--format=%H%x1f%s%x1f%b%x1e", range])
    .split("\x1e").filter(entry => entry.trim()).map(entry => {
      const [sha, subject, body] = entry.trim().split("\x1f");
      return { subject, body, files: git(["diff-tree", "--no-commit-id", "--name-only", "-r", sha]).trim().split("\n") };
    });
  const changes = curatedJson
    ? validateCuratedChanges(JSON.parse(curatedJson)).changes
    : changesFromCommits(commits);
  validateContributorCredits(changes, commits);
  if (!changes.length) throw new Error("No user-facing release notes. Add Release-Note trailers before cutting a release.");
  const notes = JSON.parse(readFileSync("release-notes.json", "utf8"));
  if (notes.some(note => note.version === version)) throw new Error(`release notes already contain ${version}`);
  const release = { version, date: new Date().toISOString().slice(0, 10), changes };
  notes.unshift(release);
  writeFileSync("release-notes.json", `${JSON.stringify(notes, null, 2)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  generateReleaseNotes(process.env.VERSION, process.env.PREVIOUS, process.env.SOURCE_REF);
}
