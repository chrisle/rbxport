import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { sentryVitePlugin } from "@sentry/vite-plugin";
import { fileURLToPath, URL } from "node:url";
import process from "node:process";
import { readFileSync } from "node:fs";
import JavaScriptObfuscator from "javascript-obfuscator";

const host = process.env.TAURI_DEV_HOST;
const packageVersion = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;
const sentryAuthToken = process.env.SENTRY_AUTH_TOKEN;
const sentryRelease = `rbxport@${packageVersion}`;

export default defineConfig({
  define: { __APP_VERSION__: JSON.stringify(packageVersion) },
  plugins: [react(), {
    name: "obfuscate-production",
    apply: "build",
    enforce: "post",
    // Transform before Rollup finalizes hashes and cross-chunk imports.
    renderChunk(code, chunk) {
      if (chunk.name === "vendor") return null;
      const obfuscated = JavaScriptObfuscator.obfuscate(code, {
        target: "browser-no-eval",
        seed: 1,
        compact: true,
        sourceMap: Boolean(sentryAuthToken),
        sourceMapMode: "separate",
        sourceMapSourcesMode: "sources-content",
        inputFileName: chunk.fileName,
        identifierNamesGenerator: "hexadecimal",
        renameGlobals: false,
        renameProperties: false,
        // Vite discovers lazy chunks' CSS dependencies after renderChunk.
        // Keep import paths literal so that discovery survives obfuscation.
        ignoreImports: true,
        // Preserve CSP and keep the deck/canvas hot paths inexpensive.
        controlFlowFlattening: false,
        deadCodeInjection: false,
        debugProtection: false,
        selfDefending: false,
        disableConsoleOutput: false,
        stringArray: true,
        stringArrayThreshold: 0.5,
        stringArrayEncoding: [],
      });
      return {
        code: obfuscated.getObfuscatedCode(),
        map: sentryAuthToken ? obfuscated.getSourceMap() : null,
      };
    },
  },
  // Source maps exist only while a release build with a scoped upload token is
  // running. They are uploaded, then removed before Tauri packages `dist/`.
  ...(sentryAuthToken ? sentryVitePlugin({
    authToken: sentryAuthToken,
    org: "triode",
    project: "rbxport",
    telemetry: false,
    release: { name: sentryRelease, inject: false },
    sourcemaps: { filesToDeleteAfterUpload: ["./dist/**/*.map"] },
  }) : [])],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
    // To evaluate Preact as a smaller compatibility layer, add:
    //   react: "preact/compat", "react-dom": "preact/compat"
  },
  clearScreen: false,
  build: {
    // WKWebView (macOS 13+) and WebView2 (Edge 110+) are the only targets we ship to.
    target: ["safari16", "edge110"],
    sourcemap: sentryAuthToken ? "hidden" : false,
    rollupOptions: {
      output: {
        manualChunks(id) {
          // Third-party code gains no protection from obfuscation.
          if (id.includes("/node_modules/")) return "vendor";
          // keep canvas + device views off the cold-start path
          if (id.includes("/src/canvas/")) return "canvas";
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
});
