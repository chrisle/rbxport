import assert from "node:assert/strict";
import test from "node:test";
import { candidateAction, checkpointStatus, journalIdentity, nextVersion, parseArgs, taggedRunAction } from "./release-cut.mjs";

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
  assert.equal(taggedRunAction([{ headSha: "a", status: "in_progress" }], "a"), "attach");
  assert.equal(taggedRunAction([{ headSha: "b", status: "in_progress" }], "a"), "block");
});

test("mocked failed tagged release reruns its immutable candidate", () => {
  assert.equal(taggedRunAction([{ headSha: "a", status: "completed", conclusion: "failure" }], "a"), "rerun");
  assert.equal(taggedRunAction([{ headSha: "a", status: "completed", conclusion: "success" }], "a"), "verify");
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
