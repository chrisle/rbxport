import assert from "node:assert/strict";
import test from "node:test";

import { planValidation, validationKeys } from "./validation-plan.mjs";

function enabled(...paths) {
  const plan = planValidation(paths);
  return validationKeys.filter(key => plan[key]);
}

test("documentation-only changes require no expensive validation lane", () => {
  assert.deepEqual(enabled("README.md", "docs/testing.md", ".github/workflows/README.md"), []);
});

test("a Rust library change stays in the focused Rust lane", () => {
  assert.deepEqual(enabled("crates/rbl-db/src/export.rs"), ["rust"]);
});

test("desktop shell changes use focused Rust checks before deploy", () => {
  assert.deepEqual(enabled("src-tauri/src/lib.rs"), ["rust"]);
});

test("Windows installer resources add the Windows-native check", () => {
  assert.deepEqual(enabled("src-tauri/windows/install-update-task.ps1"), ["rust", "windows_rust"]);
});

test("frontend test-only changes skip browser and bundle checks", () => {
  assert.deepEqual(enabled("src/lib/cues.test.ts"), [
    "frontend_lint",
    "frontend_typecheck",
    "frontend_unit",
  ]);
});

test("frontend source changes use the fast frontend checks", () => {
  assert.deepEqual(enabled("src/views/player/useGridEditor.ts"), [
    "frontend_lint",
    "frontend_typecheck",
    "frontend_unit",
  ]);
});

test("Playwright-only changes run lint, build, and browser tests", () => {
  assert.deepEqual(enabled("e2e/grid-edit.spec.ts"), [
    "frontend_lint",
    "frontend_typecheck",
    "frontend_build",
    "frontend_e2e",
  ]);
});

test("file-drop bridge changes add the Windows-native check", () => {
  assert.deepEqual(enabled("src/ipc/file-drop.ts"), [
    "frontend_lint",
    "frontend_typecheck",
    "frontend_unit",
    "windows_file_drop",
  ]);
});

test("release scripts use the focused Node script suite", () => {
  assert.deepEqual(enabled("scripts/generate-release-notes.mjs"), ["scripts"]);
});

test("the Vitest-based cleanup script also runs frontend unit tests", () => {
  assert.deepEqual(enabled("scripts/cleanup.mjs"), ["frontend_unit", "scripts"]);
});

test("packaging action changes defer expensive runtime checks to deploy", () => {
  assert.deepEqual(enabled(".github/actions/package-installer/action.yml"), ["scripts"]);
});

test("a lane workflow change exercises only that lane on the PR", () => {
  assert.deepEqual(enabled(".github/workflows/validate-windows.yml"), [
    "scripts",
    "windows_rust",
    "windows_file_drop",
  ]);
});

test("manual validation enables every lane", () => {
  assert.deepEqual(
    validationKeys.filter(key => planValidation([], { all: true })[key]),
    validationKeys,
  );
});
