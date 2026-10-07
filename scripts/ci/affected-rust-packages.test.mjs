import assert from "node:assert/strict";
import test from "node:test";

import { affectedRustPackages } from "./affected-rust-packages.mjs";

const metadata = {
  workspace_root: "/repo",
  workspace_members: ["core-id", "db-id", "app-id", "unrelated-id"],
  packages: [
    {
      id: "core-id",
      name: "rbl-core",
      manifest_path: "/repo/crates/rbl-core/Cargo.toml",
      dependencies: [],
    },
    {
      id: "db-id",
      name: "rbl-db",
      manifest_path: "/repo/crates/rbl-db/Cargo.toml",
      dependencies: [{ name: "rbl-core" }],
    },
    {
      id: "app-id",
      name: "rbxport-lib",
      manifest_path: "/repo/src-tauri/Cargo.toml",
      dependencies: [{ name: "rbl-db" }],
    },
    {
      id: "unrelated-id",
      name: "rbl-audio",
      manifest_path: "/repo/crates/rbl-audio/Cargo.toml",
      dependencies: [],
    },
  ],
};

test("includes changed crates and every transitive reverse dependent", () => {
  assert.deepEqual(affectedRustPackages(metadata, ["crates/rbl-core/src/lib.rs"]), [
    "rbl-core",
    "rbl-db",
    "rbxport-lib",
  ]);
});

test("does not include unrelated workspace packages", () => {
  assert.deepEqual(affectedRustPackages(metadata, ["crates/rbl-audio/src/lib.rs"]), [
    "rbl-audio",
  ]);
});

test("workspace manifests require the complete workspace", () => {
  assert.equal(affectedRustPackages(metadata, ["Cargo.lock"]), null);
});

test("desktop shell changes target the application package", () => {
  assert.deepEqual(affectedRustPackages(metadata, ["src-tauri/src/lib.rs"]), ["rbxport-lib"]);
});

test("unowned Rust files fall back to the complete workspace", () => {
  assert.equal(affectedRustPackages(metadata, ["build.rs"]), null);
});
