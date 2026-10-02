import assert from "node:assert/strict";
import test from "node:test";
import { candidateAction, nextVersion, parseArgs } from "./release-cut.mjs";

test("chooses a new rc exactly once from a tagged source", () => {
  assert.deepEqual(candidateAction({ sourceVersion: "1.0.0-rc.16", tagsAtHead: ["v1.0.0-rc.16"] }), {
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
