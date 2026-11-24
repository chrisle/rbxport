import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { cleanRunner } from "../../.github/actions/clean-runner/cleanup.mjs";

test("cleanup removes generated output and per-job cargo cache, retaining source", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "runner-cleanup-"));
  try {
    const workspace = path.join(root, "workspace");
    const runnerTemp = path.join(root, "temp");
    const cargoHome = path.join(runnerTemp, "cargo-home");
    for (const name of ["target", "dist", "test-results", "playwright-report", "dist-mock", "src"]) {
      await mkdir(path.join(workspace, name), { recursive: true });
      await writeFile(path.join(workspace, name, "file"), name);
    }
    await mkdir(cargoHome, { recursive: true });
    await writeFile(path.join(cargoHome, "dependency"), "cached");
    // A cache post step runs before this cleanup post step.
    assert.equal(await readFile(path.join(workspace, "target/file"), "utf8"), "target");
    assert.equal(await readFile(path.join(cargoHome, "dependency"), "utf8"), "cached");
    await cleanRunner({ workspace, runnerTemp, cargoHome, log() {} });
    for (const name of ["target", "dist", "test-results", "playwright-report", "dist-mock"]) {
      await assert.rejects(readFile(path.join(workspace, name, "file")), { code: "ENOENT" });
    }
    await assert.rejects(readFile(path.join(cargoHome, "dependency")), { code: "ENOENT" });
    assert.equal(await readFile(path.join(workspace, "src/file"), "utf8"), "src");
    await cleanRunner({ workspace, runnerTemp, cargoHome, log() {} });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("cleanup never removes a shared cargo home or the whole runner temp directory", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "runner-cleanup-"));
  try {
    const workspace = path.join(root, "workspace");
    const runnerTemp = path.join(root, "temp");
    const shared = path.join(root, "shared-cargo");
    for (const directory of [workspace, runnerTemp, shared]) await mkdir(directory);
    await writeFile(path.join(shared, "keep"), "shared");
    await writeFile(path.join(runnerTemp, "keep"), "temporary neighbor");
    const link = path.join(runnerTemp, "shared-link");
    await symlink(shared, link, process.platform === "win32" ? "junction" : "dir");
    for (const cargoHome of [shared, runnerTemp, path.join(runnerTemp, ".."), link]) {
      await cleanRunner({ workspace, runnerTemp, cargoHome, log() {} });
    }
    assert.equal(await readFile(path.join(shared, "keep"), "utf8"), "shared");
    assert.equal(await readFile(path.join(runnerTemp, "keep"), "utf8"), "temporary neighbor");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
