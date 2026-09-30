# Sentry

RBXport sends unhandled renderer errors plus native Rust panics and internal
errors. Backend error messages are deliberately omitted because they can
contain library paths or track metadata. It does not enable performance
tracing, session replay, release health, user identity, request data, or
breadcrumbs.

The DSN is supplied to both parts of the desktop app at build time. The scoped
`Sentry auth token (source maps)` is used only by the Vite build to upload
hidden source maps for the matching `rbxport@<version>` release; those maps are
deleted before Tauri packages `dist/`. After both items exist in `rbxport.com`:

```sh
cp .env.sentry.example .env.sentry
/Users/chrisle/code/skills/1password/scripts/opx root item get "Sentry DSN" --vault rbxport.com --format json >/dev/null
/Users/chrisle/code/skills/1password/scripts/opx vault rbxport.com run --env-file .env.sentry -- pnpm tauri build
```

The first command only verifies access. `op run` resolves the two `op://`
references for the build. `.env.sentry` is ignored by Git; the built-in Sentry
client is disabled when either DSN is absent.
