import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { candidateAction, checkpointStatus, journalIdentity, nextVersion, parseArgs, reconcileCheckpoint, taggedRunAction } from "./release-cut.mjs";

test("chooses a new rc exactly once from a tagged source", () => {
  assert.deepEqual(candidateAction({ sourceVersion: "1.0.0-rc.16", tagsAtHead: ["v1.0.0-rc.16"] }), {
    action: "prepare", version: "1.0.0-rc.17",
  });
});

test("chooses the next rc when dev is ahead of an older source-version tag", () => {
  assert.deepEqual(candidateAction({ sourceVersion: "1.0.0-rc.16", tagsAtHead: [], usedTags: ["v1.0.0-rc.16"] }), {
    action: "prepare", version: "1.0.0-rc.17",
  });
});

test("resumes an untagged candidate without another version bump", () => {
  assert.deepEqual(candidateAction({ sourceVersion: "1.0.0-rc.17", tagsAtHead: [], resume: true }), {
    action: "resume", version: "1.0.0-rc.17",
  });
});

test("resumes a tagged candidate after a crash without creating another tag", () => {
  assert.deepEqual(candidateAction({ sourceVersion: "1.0.0-rc.17", tagsAtHead: ["v1.0.0-rc.17"], resume: true }), {
    action: "resume-tag", version: "1.0.0-rc.17",
  });
});

test("rejects ambiguous untagged candidates and invalid notes", () => {
  assert.throws(() => candidateAction({ sourceVersion: "1.0.0-rc.17", tagsAtHead: [] }), /--resume/);
  assert.throws(() => parseArgs(["--note", "release now"]), /must start/);
});

test("dry runs cannot also resume a publish candidate", () => {
  assert.throws(() => parseArgs(["--dry-run", "--resume"]), /cannot be combined/);
});

test("checkpoint recovery refuses edits even to release-owned files", () => {
  const before = { "Cargo.toml": "a", "Cargo.lock": "b" };
  const after = { "Cargo.toml": "c", "Cargo.lock": "d" };
  assert.equal(checkpointStatus({ state: "preparing", before }, before), "retry");
  assert.equal(checkpointStatus({ state: "prepared", after }, after), "commit");
  assert.equal(checkpointStatus({ state: "prepared", after }, { ...after, "Cargo.toml": "user" }), "refuse");
});

test("mocked tagged release attaches only to its matching active run", () => {
  assert.equal(taggedRunAction([{ headSha: "a", headBranch: "v1.0.0-rc.17", status: "in_progress" }], "a", "v1.0.0-rc.17"), "attach");
  assert.equal(taggedRunAction([{ headSha: "a", headBranch: "vother", status: "in_progress" }], "a", "v1.0.0-rc.17"), "block");
});

test("mocked failed tagged release reruns its immutable candidate", () => {
  assert.equal(taggedRunAction([{ headSha: "a", headBranch: "v1.0.0-rc.17", status: "completed", conclusion: "failure" }], "a", "v1.0.0-rc.17"), "rerun");
  assert.equal(taggedRunAction([{ headSha: "a", headBranch: "v1.0.0-rc.17", status: "completed", conclusion: "success" }], "a", "v1.0.0-rc.17"), "verify");
});

test("mocked journal identity blocks stale sha and manifest version", () => {
  const record = { sha: "a", version: "1.0.0-rc.17" };
  assert.equal(journalIdentity(record, { head: "a", version: record.version, originDev: "a" }), true);
  assert.equal(journalIdentity(record, { head: "b", version: record.version, originDev: "b" }), false);
  assert.equal(journalIdentity(record, { head: "a", version: "1.0.0-rc.18", originDev: "a" }), false);
});

test("mocked partial prepare is preserved and fails closed", () => {
  const before = { "Cargo.toml": "a" };
  assert.equal(checkpointStatus({ state: "preparing", before }, { "Cargo.toml": "partial" }), "refuse");
});

test("mocked crash checkpoints reconcile only known commit and pre-ff states", () => {
  assert.equal(reconcileCheckpoint({ state: "prepared", head: "candidate", candidate: "candidate" }), "committed");
  assert.equal(reconcileCheckpoint({ state: "switching-main", head: "old-main", candidate: "candidate", main: "old-main" }), "fast-forward-main");
});

function git(cwd, args) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

function releaseFixture(mode) {
  const root = mkdtempSync(join(tmpdir(), "rbxport-release-cut-"));
  const remote = join(root, "remote.git");
  const repo = join(root, "repo");
  const bin = join(root, "bin");
  mkdirSync(repo); mkdirSync(bin);
  git(root, ["init", "--bare", remote]);
  git(repo, ["init", "-b", "dev"]);
  git(repo, ["config", "user.email", "tests@example.invalid"]);
  git(repo, ["config", "user.name", "release test"]);
  mkdirSync(join(repo, "scripts"));
  mkdirSync(join(repo, "src-tauri"));
  cpSync(new URL("./release-cut.mjs", import.meta.url), join(repo, "scripts/release-cut.mjs"));
  cpSync(new URL("./release-preflight.mjs", import.meta.url), join(repo, "scripts/release-preflight.mjs"));
  cpSync(new URL("./release-notes.mjs", import.meta.url), join(repo, "scripts/release-notes.mjs"));
  writeFileSync(join(repo, "Cargo.toml"), "[workspace.package]\nversion = \"1.0.0-rc.17\"\n");
  writeFileSync(join(repo, "Cargo.lock"), "# lock\n");
  writeFileSync(join(repo, "package.json"), '{"version":"1.0.0-rc.17"}\n');
  writeFileSync(join(repo, "src-tauri/tauri.conf.json"), '{"version":"1.0.0-rc.17"}\n');
  writeFileSync(join(repo, "release-notes.json"), '[{"version":"1.0.0-rc.17","date":"2026-10-02","changes":["(Fixed) recovery"]}]\n');
  writeFileSync(join(repo, "base.txt"), "base\n");
  git(repo, ["add", "."]); git(repo, ["commit", "-m", "base"]);
  git(repo, ["remote", "add", "origin", remote]); git(repo, ["push", "-u", "origin", "dev"]);
  git(repo, ["switch", "-c", "main"]); git(repo, ["push", "-u", "origin", "main"]); git(repo, ["switch", "dev"]);
  writeFileSync(join(repo, "candidate.txt"), "candidate\n");
  git(repo, ["add", "candidate.txt"]); git(repo, ["commit", "-m", "chore(release): prepare 1.0.0-rc.17"]);
  const sha = git(repo, ["rev-parse", "HEAD"]);
  git(repo, ["push", "origin", "dev"]);
  const log = join(root, "commands.log");
  writeFileSync(join(bin, "pnpm"), '#!/bin/sh\necho "pnpm $*" >> "$MOCK_LOG"\n[ "$MOCK_PNPM" = fail ] && exit 23\nexit 0\n', { mode: 0o755 });
  writeFileSync(join(bin, "gh"), `#!/bin/sh
echo "gh $*" >> "$MOCK_LOG"
case "$*" in
  *"run list"*"--workflow Validate"*) echo '[{"databaseId":11,"headSha":"'"$MOCK_SHA"'","status":"completed"}]' ;;
  *"run list"*"--workflow Release"*"--limit 30"*) cat "$MOCK_OUTER" ;;
  *"run list"*"--workflow Release"*"--event workflow_dispatch"*) echo '[{"databaseId":22,"headSha":"'"$MOCK_SHA"'","status":"completed"}]' ;;
  *"run list"*"--workflow Release"*) cat "$MOCK_RELEASE" ;;
  *"run watch 11"*) exit 0 ;;
  *"run watch"*) exit 41 ;;
  *) exit 0 ;;
esac
`, { mode: 0o755 });
  return { root, repo, sha, log, bin, mode };
}

function runFixture(fixture, { outer = [], release = [] } = {}) {
  const outerPath = join(fixture.root, "outer.json");
  const releasePath = join(fixture.root, "release.json");
  writeFileSync(outerPath, JSON.stringify(outer));
  writeFileSync(releasePath, JSON.stringify(release));
  try {
    execFileSync(process.execPath, ["scripts/release-cut.mjs", "--resume"], {
      cwd: fixture.repo,
      encoding: "utf8",
      env: { ...process.env, PATH: `${fixture.bin}:${process.env.PATH}`, MOCK_LOG: fixture.log, MOCK_SHA: fixture.sha, MOCK_OUTER: outerPath, MOCK_RELEASE: releasePath, MOCK_PNPM: fixture.mode === "stop-after-recovery" ? "fail" : "ok" },
    });
  } catch (error) {
    return `${error.stdout ?? ""}${error.stderr ?? ""}`;
  }
  return "";
}

function journal(repo, value) { writeFileSync(join(repo, ".git/rbxport-release-cut.json"), `${JSON.stringify(value)}\n`); }

test("command path recovers a commit made before its journal checkpoint", () => {
  const fixture = releaseFixture("stop-after-recovery");
  try {
    journal(fixture.repo, { version: "1.0.0-rc.17", state: "prepared", after: {} });
    // The crash point has a clean release commit but an older prepared journal.
    const result = runFixture(fixture);
    assert.match(result, /pnpm release:preflight -- --expect-untagged failed/);
    assert.equal(JSON.parse(readFileSync(join(fixture.repo, ".git/rbxport-release-cut.json"))).state, "pushed");
    assert.equal(git(fixture.repo, ["log", "-1", "--format=%s"]), "chore(release): prepare 1.0.0-rc.17");
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("command path fast-forwards main after a pre-merge interruption", () => {
  const fixture = releaseFixture("stop-after-recovery");
  try {
    git(fixture.repo, ["switch", "main"]);
    journal(fixture.repo, { version: "1.0.0-rc.17", sha: fixture.sha, state: "switching-main" });
    const result = runFixture(fixture);
    assert.match(result, /pnpm release:preflight -- --expect-untagged failed/);
    assert.equal(git(fixture.repo, ["rev-parse", "HEAD"]), fixture.sha);
    assert.equal(git(fixture.repo, ["rev-parse", "origin/main"]), fixture.sha);
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("command path pushes an already-created local release tag", () => {
  const fixture = releaseFixture("release-active");
  try {
    git(fixture.repo, ["switch", "main"]); git(fixture.repo, ["merge", "--ff-only", fixture.sha]); git(fixture.repo, ["push", "origin", "main"]);
    git(fixture.repo, ["tag", "-a", "v1.0.0-rc.17", "-m", "test", fixture.sha]);
    journal(fixture.repo, { version: "1.0.0-rc.17", sha: fixture.sha, state: "main" });
    const active = [{ databaseId: 22, headSha: fixture.sha, headBranch: "v1.0.0-rc.17", status: "in_progress" }];
    const result = runFixture(fixture, { release: active });
    assert.match(result, /gh run watch 22 --exit-status failed/);
    assert.equal(git(fixture.repo, ["ls-remote", "--tags", "origin", "refs/tags/v1.0.0-rc.17^{}"]), `${fixture.sha}\trefs/tags/v1.0.0-rc.17^{}`);
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});

test("command path blocks foreign active releases and redispatches its exact tag", () => {
  const fixture = releaseFixture("release-active");
  try {
    git(fixture.repo, ["switch", "main"]); git(fixture.repo, ["merge", "--ff-only", fixture.sha]); git(fixture.repo, ["push", "origin", "main"]);
    git(fixture.repo, ["tag", "-a", "v1.0.0-rc.17", "-m", "test", fixture.sha]); git(fixture.repo, ["push", "origin", "v1.0.0-rc.17"]);
    journal(fixture.repo, { version: "1.0.0-rc.17", sha: fixture.sha, state: "tagged" });
    const foreign = [{ databaseId: 99, headSha: "other", headBranch: "vother", status: "in_progress" }];
    assert.match(runFixture(fixture, { outer: foreign }), /a release is already active/);
    assert.doesNotMatch(readFileSync(fixture.log, "utf8"), /workflow run Release/);
    writeFileSync(fixture.log, "");
    const failed = [{ databaseId: 77, headSha: fixture.sha, headBranch: "v1.0.0-rc.17", status: "completed", conclusion: "failure" }];
    assert.match(runFixture(fixture, { release: failed }), /gh run watch 22 --exit-status failed/);
    assert.match(readFileSync(fixture.log, "utf8"), /workflow run Release --ref v1\.0\.0-rc\.17 -f release_tag=v1\.0\.0-rc\.17/);
  } finally { rmSync(fixture.root, { recursive: true, force: true }); }
});
