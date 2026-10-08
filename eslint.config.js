/**
 * Lint rules that enforce the frontend architecture.
 *
 * The interesting half is at the bottom: the restrictions that encode this
 * project's architecture — Rust owns everything list-shaped, `invoke` lives
 * only in `src/ipc/`, nothing polls. A rule here catches those on the way in
 * rather than in review.
 */
import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import tseslint from "typescript-eslint";

/**
 * Syntax nobody should reach for anywhere in the frontend.
 *
 * Held in a constant because a later config block that sets
 * `no-restricted-syntax` *replaces* this list rather than adding to it — the
 * views block below would otherwise silently drop all of these.
 */
const BANNED_SYNTAX = [
  {
    selector: "CallExpression[callee.name='setInterval']",
    message: "No polling: idle CPU is budgeted at 0.5%. Use an event or a ResizeObserver.",
  },
  {
    selector: "CallExpression[callee.name='structuredClone']",
    message: "Deep-cloning a row array is the copy the columnar index exists to avoid.",
  },
  {
    selector:
      "CallExpression[callee.object.name='JSON'][callee.property.name='parse'] > CallExpression[callee.object.name='JSON'][callee.property.name='stringify']",
    message: "JSON.parse(JSON.stringify(x)) is a deep clone in disguise.",
  },
  {
    // tauri-plugin-dialog replaces these with calls to plugin commands it no
    // longer registers, so they reject instead of asking (#107, #86).
    selector:
      "CallExpression[callee.object.name='window'][callee.property.name=/^(alert|confirm|prompt)$/], CallExpression[callee.name=/^(alert|confirm|prompt)$/]",
    message: "Use getBackend().confirm (the native dialog); window dialogs fail in the Tauri app.",
  },
];

export default tseslint.config(
  { ignores: ["dist", "design/sketch/dist", "src-tauri/target", "coverage"] },
  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
      globals: { window: "readonly", document: "readonly", console: "readonly" },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,

      // A leading underscore is how this codebase says "required by the
      // signature, deliberately unused".
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],

      // Debug output that reached a user's console is noise; a warning or an
      // error is a thing someone meant to say.
      "no-console": ["error", { allow: ["warn", "error"] }],

      "no-restricted-syntax": ["error", ...BANNED_SYNTAX],
    },
  },
  {
    // `invoke` only inside src/ipc: every view calls a typed wrapper, so the
    // IPC surface stays auditable and the mock can stand in wholesale.
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/ipc/**"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@tauri-apps/api/core",
              message: "invoke belongs in src/ipc/. Views call the typed wrappers there.",
            },
          ],
        },
      ],
    },
  },
  {
    // Rust owns sorting, filtering and searching. A view that sorts an array
    // is sorting the fetched window, which is the wrong answer shown quickly.
    files: ["src/views/**/*.{ts,tsx}"],
    rules: {
      "no-restricted-syntax": [
        "error",
        ...BANNED_SYNTAX,
        {
          selector:
            "CallExpression[callee.property.name=/^(sort|filter)$/][callee.object.name=/^(rows|tracks|items)$/]",
          message: "Rust orders and filters the list; a view only renders the window it fetched.",
        },
      ],
    },
  },
  {
    // Tests say what they mean; the strictness that helps the app gets in the
    // way of a fixture.
    files: ["**/*.test.ts", "**/*.test.tsx", "e2e/**/*.ts", "scripts/**/*.ts", "design/**/*.ts"],
    rules: {
      "@typescript-eslint/no-unsafe-assignment": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
      "@typescript-eslint/no-unsafe-call": "off",
      "no-console": "off",
    },
  },
);
