import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { lanesAlwaysRun, tiers } from "./test-tiers.mjs";
import { planValidation, validationKeys } from "./validation-plan.mjs";

const doc = readFileSync(new URL("../../docs/development/test-tiers.md", import.meta.url), "utf8");

// One path that makes the pull-request plan enable each lane. A lane with no
// entry here, or one the planner no longer reaches, is a lane that can only
// ever run at release.
const reachedBy = {
  frontend_lint: "src/lib/cues.ts",
  frontend_typecheck: "src/lib/cues.ts",
  frontend_build: "index.html",
  frontend_unit: "src/lib/cues.ts",
  frontend_e2e: "e2e/library.spec.ts",
  frontend_budget: "perf-budgets.json",
  scripts: "scripts/release-preflight.mjs",
  rust: "crates/rbl-db/src/export.rs",
  windows_rust: "src-tauri/windows/install-update-task.ps1",
  windows_file_drop: "src/ipc/file-drop.ts",
  linux_window: ".github/workflows/validate-linux.yml",
  macos_intel: ".github/workflows/validate-macos.yml",
};

test("every validation lane has a path that selects it on a pull request", () => {
  assert.deepEqual(Object.keys(reachedBy).sort(), [...validationKeys].sort());
  for (const lane of validationKeys) {
    assert.equal(planValidation([reachedBy[lane]])[lane], true, `${lane} via ${reachedBy[lane]}`);
  }
});

test("a tier only names lanes the planner knows", () => {
  for (const tier of tiers) {
    for (const lane of tier.lanes) assert.ok(validationKeys.includes(lane), `${tier.id}: ${lane}`);
  }
});

test("every lane runs unconditionally in an active tier, so a skipped PR path never loses it", () => {
  const always = lanesAlwaysRun();
  for (const lane of validationKeys) assert.ok(always.has(lane), `${lane} is never run in full`);
});

test("the policy document names every tier and every lane", () => {
  for (const tier of tiers) assert.ok(doc.includes(`\`${tier.id}\``), `tier ${tier.id} missing from the doc`);
  for (const lane of validationKeys) assert.ok(doc.includes(`\`${lane}\``), `lane ${lane} missing from the doc`);
});

test("planned tiers are labelled as planned in the document", () => {
  for (const tier of tiers.filter(candidate => candidate.status === "planned")) {
    const row = doc.split("\n").find(line => line.includes(`\`${tier.id}\``) && line.startsWith("|"));
    assert.ok(row?.toLowerCase().includes("planned"), `${tier.id} must say planned in its table row`);
  }
});
