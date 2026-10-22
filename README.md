<p align="center">
  <a href="https://rbxport.com">
    <img src="docs/assets/brand/combined/rbxport-logo-color-white.png" alt="rbxport" width="360">
  </a>
</p>

# rbxport

rbxport aims to be a fast rekordbox clone with a deliberately limited feature
set. It prioritizes speed and responsiveness, including on older machines.

Its scope is limited to library management, USB exporting, and PRO DJ LINK.

## Project vision

rbxport trades feature breadth for speed. It focuses on a limited set of
rekordbox workflows: library management, USB exporting, and PRO DJ LINK. Within
that scope, it aims to reproduce rekordbox's behavior and compatibility.

The core project does not aim to become a full DJ performance application or
an advanced library manager beyond what rekordbox offers. A feature having a
rekordbox counterpart is necessary for inclusion, but does not guarantee it
belongs in this focused subset. See the [product decisions](docs/development/product-decisions.md)
for specific scope decisions.

rbxport also aims to be a useful foundation for others to fork and build their
own DJ software. Forks can pursue broader features and different product goals
while the core project stays focused and fast.

Two commitments define what "small" must not cost:

- **Compatibility.** rbxport aims to be fully compatible with the current
  version of rekordbox and all the hardware that rekordbox supports.
- **Performance.** rbxport maintains its speed through a performance budget,
  defined in `perf-budgets.json` and enforced by `pnpm budget` and CI. See
  [Development conventions](docs/development/conventions.md).

If you want to propose a feature, read
[Feature proposals](CONTRIBUTING.md#feature-proposals) in the contributing guide
first.

## Tech stack

| Layer | Technology |
| --- | --- |
| Desktop app | Tauri 2. |
| Frontend | React and TypeScript, with TanStack Virtual for track browsing. |
| Backend | Rust, with independent `rbl-*` crates for library, audio, export, and LINK logic. |
| Database | SQLCipher for rekordbox libraries and OneLibrary USB exports. |
| Build tooling | Vite, pnpm, and Cargo. |
| Testing | Vitest, Playwright, and Rust tests with temporary library fixtures. |

See [Architecture](docs/development/architecture.md) for the repository layout
and how the frontend, desktop shell, and Rust crates fit together.

## Start here

For a first look at the interface, use the browser with a mock library:

```sh
pnpm install
pnpm dev:web
```

This requires Node 24 and pnpm 10.17.1. It does not open an installed rekordbox
library or require a Rust build. For desktop prerequisites and safe library
setup, read [Getting started](docs/development/getting-started.md).

## Developer reading path

Read these in order to understand the project and make your first change:

1. [Getting started](docs/development/getting-started.md): prerequisites, run modes, and library safety.
2. [Architecture](docs/development/architecture.md): repository map, crate responsibilities, IPC, and edit flow.
3. [Development conventions](docs/development/conventions.md): code style, performance, translation, and evidence rules.
4. [Testing](docs/development/testing.md): checks, test setup, and what each result proves.
5. [Contributing](CONTRIBUTING.md): issues, branches, implementation, validation, and review.
6. [Debugging](docs/development/debugging.md): logs, diagnostics, environment variables, and cleanup.
7. [Releases](docs/development/releases.md): versions, CI, packaging, and publication.

The [documentation index](docs/README.md) also links to user guides and detailed
analysis, USB-format, and hardware references.

## Common commands

| Command | Purpose |
| --- | --- |
| `pnpm dev:web` | Browser UI with the mock backend. |
| `pnpm dev` | Desktop app; can open your installed library. Read the safety guide first. |
| `pnpm lint` | Frontend lint rules. |
| `pnpm build` | TypeScript check and production frontend bundle. |
| `pnpm test` | Vitest tests. |
| `pnpm e2e` | Playwright browser tests against the mock backend. |
| `RB_LITE_TEST=1 cargo test --workspace` | Rust tests with installed-library writes refused. |

## License and trademarks

rbxport is GPL-2.0-or-later. See [LICENSE](LICENSE) and
[Licensing](LICENSING.md) for bundled components and distribution terms.
Third-party product names belong to their respective owners. rbxport is an
independent project and is not affiliated with, endorsed by, or sponsored by
their owners.

## Disclaimer

rbxport is provided "as is", without warranty to the extent permitted by
applicable law. Use it at your own risk and keep backups of your music library
and USB drives. Unless required by
applicable law or agreed to in writing, the authors and contributors are not
liable for damages arising from using or being unable to use rbxport,
including lost or corrupted data, equipment damage, or financial losses.
See [LICENSE](LICENSE) for the full warranty and liability terms.

rbxport is an independent project and is not affiliated with, endorsed by,
or sponsored by AlphaTheta or Pioneer DJ. References to rekordbox,
PRO DJ LINK, and other products describe compatibility only. All trademarks
and product names belong to their respective owners.

## Special thanks

evanpurkhiser, Maddix, Morgan Page, nichi, profbx, Sean Tyas, shiz, syl, trancejesus, xorbxbx, and
AlphaTheta.
