import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const validationKeys = [
  "frontend_lint",
  "frontend_typecheck",
  "frontend_build",
  "frontend_unit",
  "frontend_e2e",
  "frontend_budget",
  "scripts",
  "rust",
  "windows_rust",
  "windows_file_drop",
  "linux_window",
  "macos_intel",
];

function emptyPlan() {
  return Object.fromEntries(validationKeys.map(key => [key, false]));
}

function enable(plan, ...keys) {
  for (const key of keys) plan[key] = true;
}

function enableFrontend(plan) {
  enable(
    plan,
    "frontend_lint",
    "frontend_typecheck",
    "frontend_unit",
  );
}

export function planValidation(paths, { all = false } = {}) {
  const plan = emptyPlan();
  if (all) {
    enable(plan, ...validationKeys);
    return plan;
  }

  for (const rawPath of paths) {
    const path = rawPath.replace(/^\.\//, "").trim();
    if (!path) continue;

    // PR checks stay intentionally focused; deploy runs the entire graph.
    // A lane's own workflow change exercises that lane, while orchestration
    // and packaging changes rely on actionlint here and the full deploy gate.
    if (/^\.github\/workflows\/.*\.ya?ml$/.test(path)) {
      enable(plan, "scripts");
      if (path.endsWith("validate-rust.yml")) enable(plan, "rust");
      if (path.endsWith("validate-windows.yml")) {
        enable(plan, "windows_rust", "windows_file_drop");
      }
      if (path.endsWith("validate-linux.yml")) enable(plan, "linux_window");
      if (path.endsWith("validate-macos.yml")) enable(plan, "macos_intel");
      if (path.endsWith("validate-frontend.yml")) enableFrontend(plan);
      if (path.endsWith("validation.yml") || path.endsWith("ci.yml")) {
        enableFrontend(plan);
        enable(plan, "rust");
      }
      continue;
    }

    if (/^\.github\/actions\//.test(path)) {
      enable(plan, "scripts");
      continue;
    }

    if (/^(Cargo\.toml|Cargo\.lock|rust-toolchain(?:\.toml)?|\.cargo\/)/.test(path)) {
      enable(plan, "rust");
      continue;
    }

    // Backups flush and rename files, which Windows handles differently.
    if (/^(src-tauri\/src\/backup|crates\/rbl-backup\/|crates\/rbl-core\/src\/durable\.rs$)/.test(path)) {
      enable(plan, "rust", "windows_rust");
      continue;
    }

    if (path.startsWith("src-tauri/")) {
      enable(plan, "rust");
      // Installer hooks and scripts only ever run on Windows. The file-drop
      // bridge has a cfg(windows) WebView2 half and the manifest holds the
      // cfg(windows) dependencies; the Linux lane compiles neither.
      if (
        path.startsWith("src-tauri/windows/") ||
        path === "src-tauri/src/file_drop.rs" ||
        path === "src-tauri/Cargo.toml"
      ) {
        enable(plan, "windows_rust");
      }
      continue;
    }

    if (path.endsWith(".rs") || path.endsWith(".toml") || /^(crates|iTunes)\//.test(path)) {
      enable(plan, "rust");
      continue;
    }

    if (/^(package\.json|pnpm-lock\.yaml|eslint\.config\.[cm]?[jt]s|vitest\.config\.[cm]?[jt]s|vite\.config\.[cm]?[jt]s|tsconfig(?:\.[^.]+)?\.json)$/.test(path)) {
      enableFrontend(plan);
      enable(plan, "frontend_build");
      enable(plan, "scripts");
      continue;
    }

    if (/^playwright\.config\.[cm]?[jt]s$/.test(path)) {
      enable(plan, "frontend_lint", "frontend_typecheck", "frontend_build", "frontend_e2e");
      continue;
    }

    if (path.startsWith("src/")) {
      enable(plan, "frontend_lint", "frontend_typecheck", "frontend_unit");
      if (/^src\/ipc\/file-drop(?:\.|\/)/.test(path)) enable(plan, "windows_file_drop");
      continue;
    }

    if (path.startsWith("e2e/")) {
      enable(plan, "frontend_lint", "frontend_typecheck", "frontend_build", "frontend_e2e");
      continue;
    }

    if (/^(public\/|index\.html$)/.test(path)) {
      enable(plan, "frontend_build");
      continue;
    }

    if (path.startsWith("design/")) {
      enable(plan, "frontend_typecheck", "frontend_unit");
      continue;
    }

    if (path.startsWith("scripts/")) {
      enable(plan, "scripts");
      if (/^scripts\/cleanup(?:\.test)?\.mjs$/.test(path)) enable(plan, "frontend_unit");
      if (/\.[cm]?ts$/.test(path)) enable(plan, "frontend_typecheck");
      if (/^scripts\/check-(?:bundle-size|perf-budgets)\./.test(path)) {
        enable(plan, "frontend_build", "frontend_budget");
      }
      continue;
    }

    if (path === "perf-budgets.json") {
      enable(plan, "frontend_build", "frontend_budget");
    }
  }

  return plan;
}

export function formatGithubOutput(plan) {
  const frontend = validationKeys
    .filter(key => key.startsWith("frontend_"))
    .some(key => plan[key]);
  return [
    ...validationKeys.map(key => `${key}=${plan[key]}`),
    `frontend=${frontend}`,
  ].join("\n");
}

function main() {
  const all = process.argv.includes("--all");
  const paths = all ? [] : readFileSync(0, "utf8").split(/\0|\r?\n/);
  const plan = planValidation(paths, { all });
  process.stdout.write(`${formatGithubOutput(plan)}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
