# Third-Party Licenses

Text2Create is licensed under the [Apache License 2.0](LICENSE). This document explains how third-party dependencies and their licenses are tracked.

## Current State

The application skeleton (Milestone 1) introduced the first npm and Rust dependencies. They are listed in the [Dependency Inventory](#dependency-inventory) below.

The inventory reflects **license metadata declared by each package** (npm `package.json`, Cargo `Cargo.toml`). It is a snapshot, not a legal audit, and declared licenses have not been verified against each package's source files.

Repository tooling used only in CI is listed below for transparency. These tools are not distributed with Text2Create.

| Tool | Used for | License |
| --- | --- | --- |
| [actions/checkout](https://github.com/actions/checkout) | Checking out the repository in CI | MIT |
| [actions/setup-node](https://github.com/actions/setup-node) | Installing Node.js in CI | MIT |
| [actions/cache](https://github.com/actions/cache) | Caching Cargo artifacts in CI | MIT |
| [markdownlint-cli2-action](https://github.com/DavidAnson/markdownlint-cli2-action) | Markdown linting in CI | MIT |
| [yamllint](https://github.com/adrienverge/yamllint) | YAML linting in CI | GPL-3.0 |

## Policy for Adding Dependencies

Every new dependency must be justified in its pull request, including:

1. **Purpose:** why it is needed and why existing code or dependencies are not sufficient.
2. **License:** the SPDX license identifier, verified from the dependency's source.
3. **Compatibility:** confirmation that the license is compatible with Apache 2.0 for the way the dependency is used.
4. **Maintenance:** evidence that the dependency is maintained, and its security history.
5. **Alternatives:** other options considered.

### License Compatibility Guidelines

| Category | Examples | Guidance |
| --- | --- | --- |
| Permissive | MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, ISC, Zlib | Generally acceptable |
| Weak copyleft | MPL-2.0, LGPL | Requires review of how the dependency is used and distributed |
| Strong copyleft | GPL, AGPL | Must not be linked into or bundled with Text2Create without explicit review |
| Unknown or no license | — | Not acceptable |

Some candidate engineering tools use copyleft licenses (for example KiCad and ngspice components). The intended approach is to invoke such tools as separate, user-installed programs rather than linking or bundling them. Each integration will be reviewed before it is adopted.

This document is not legal advice. When in doubt, raise the question in the pull request.

## Tracking Process

- Direct dependencies are pinned to exact versions in `package.json` and `src-tauri/Cargo.toml` / `src-tauri/Cargo.lock`. Transitive versions are pinned by `package-lock.json` and `src-tauri/Cargo.lock`.
- Every pull request that changes dependencies must update this document using the commands in [Regenerating the Inventory](#regenerating-the-inventory).
- Planned: automated license checks in CI (for example `cargo-deny` for Rust and an equivalent tool for npm). Not implemented yet.
- Planned: attribution notices bundled with distributed builds (Milestone 7). Not implemented yet.

## Dependency Inventory

Snapshot date: 2026-11-26. Platform: macOS, Apple Silicon (`aarch64-apple-darwin`).

### Direct Dependencies

| Dependency | Version | License | Category | Purpose |
| --- | --- | --- | --- | --- |
| `react` | 19.3.1 | MIT | npm runtime | User interface |
| `react-dom` | 19.3.1 | MIT | npm runtime | User interface rendering |
| `@tauri-apps/api` | 2.12.4 | Apache-2.0 OR MIT | npm runtime | Frontend-to-Rust communication |
| `@tauri-apps/cli` | 2.12.4 | Apache-2.0 OR MIT | npm dev | Tauri build and development tooling |
| `vite` | 8.3.8 | MIT | npm dev | Frontend dev server and bundler |
| `@vitejs/plugin-react` | 6.3.0 | MIT | npm dev | React support for Vite |
| `typescript` | 6.0.3 | Apache-2.0 | npm dev | Type checking |
| `@types/react` | 19.3.0 | MIT | npm dev | React type definitions |
| `@types/react-dom` | 19.3.0 | MIT | npm dev | React DOM type definitions |
| `markdownlint-cli2` | 0.24.2 | MIT | npm dev | Local Markdown linting (same version as the CI action) |
| `tauri` | 2.12.5 | Apache-2.0 OR MIT | Rust runtime | Desktop application framework |
| `tauri-build` | 2.7.4 | Apache-2.0 OR MIT | Rust build-time | Tauri build script support |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Rust runtime | Project IR serialization (`text2create-ir`) |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | Rust runtime | Project IR JSON parsing and output (`text2create-ir`) |
| `thiserror` | 2.0.21 | MIT OR Apache-2.0 | Rust runtime | Error types (`text2create-ir`) |
| `time` | 0.3.55 | MIT OR Apache-2.0 | Rust runtime | Timestamp validation (`text2create-ir`) |
| `uuid` | 1.29.3 | Apache-2.0 OR MIT | Rust runtime | Identifiers (`text2create-ir`) |
| `rusqlite` | 0.41.1 | MIT | Rust runtime | SQLite access for project persistence (`text2create-store`) |
| `serde_path_to_error` | 0.1.20 | MIT OR Apache-2.0 | Rust runtime | Locations of invalid data in error messages (`text2create-ir`) |
| SQLite (bundled by `libsqlite3-sys` 0.39.1) | 3.54.1 | Public domain | Rust runtime | Database engine, compiled from source into the application |

The `text2create-ir` dependencies were already part of the dependency tree through Tauri. `rusqlite` adds four packages: `rusqlite`, `libsqlite3-sys` (MIT), `fallible-iterator`, and `fallible-streaming-iterator` (both MIT/Apache-2.0) at runtime, and `pkg-config` and `vcpkg` (both MIT OR Apache-2.0 equivalents) at build time; it also makes `getrandom` 0.4 a runtime dependency. SQLite is in the public domain and requires no license notice. `serde_path_to_error` adds one package; its own dependencies (`itoa`, `serde`) were already present.

The default application icons in `src-tauri/icons/` come from the Tauri project template (Apache-2.0 OR MIT). They are placeholders and must be replaced before a public release.

### Categories

- **npm runtime:** production dependencies. Only code that the frontend imports is bundled into the application, so this list is an upper bound.
- **npm dev:** tooling used during development and builds. Not distributed.
- **Rust runtime:** normal dependencies excluding procedural macros, for the macOS target. These may be linked into the application binary; this is an upper bound.
- **Rust build-time:** build-script dependencies and procedural macros. They run during compilation and are not linked into the binary.
- **Rust dev:** test-only dependencies. Not distributed.

### Transitive Dependency Summary

Counts include direct dependencies and exclude the Text2Create crate itself.

| Category | Packages | Declared licenses (count) |
| --- | --- | --- |
| npm runtime | 5 | MIT (3), Apache-2.0 (1), Apache-2.0 OR MIT (1) |
| npm dev | 111 | MIT (98), ISC (3), Apache-2.0 OR MIT (2), Apache-2.0 (2), MPL-2.0 (2), BSD-3-Clause (2), PSF-2.0 (1), BSD-2-Clause (1) |
| Rust runtime | 139 | MIT OR Apache-2.0 / Apache-2.0 OR MIT and equivalent spellings (84), MIT (18), Unicode-3.0 (15), multi-license options including Zlib, Unlicense, 0BSD, or CC0-1.0 (15), BSD-3-Clause (2), BSD-3-Clause/MIT (1), BSD-3-Clause AND MIT (1), Apache-2.0 AND MIT (1), Apache-2.0 (1), MPL-2.0 (1) |
| Rust build-time | 87 | MIT OR Apache-2.0 / Apache-2.0 OR MIT and equivalent spellings (57), MIT (19), MPL-2.0 (4), Unicode-3.0 (3), other permissive (4) |
| Rust dev | 0 | — |

### Licenses Requiring Review

| Package | Version | License | Category | Assessment |
| --- | --- | --- | --- | --- |
| `option-ext` | 0.2.0 | MPL-2.0 | Rust runtime | Pulled in by `tauri` and `wry` through `dirs`. MPL-2.0 is file-level copyleft: used unmodified, it is compatible with Apache-2.0 distribution, provided the MPL-covered source remains available (it is published on crates.io) and the license notice is included in distributed builds. |
| `cssparser`, `cssparser-macros`, `dtoa-short`, `selectors` | see `Cargo.lock` | MPL-2.0 | Rust build-time | Used by Tauri's build-time HTML processing. Not linked into the binary. |
| `lightningcss`, `lightningcss-darwin-arm64` | 1.33.0 | MPL-2.0 | npm dev | CSS tooling used by Vite during builds. Not distributed. |
| `argparse` | 3.0.4 | PSF-2.0 | npm dev | Command-line parsing used by development tooling. Not distributed. |
| Unicode data crates (`icu_*` and related) | see `Cargo.lock` | Unicode-3.0 | Rust runtime | Permissive license; requires the license notice in distributed builds. |

No package with an unknown license or a strong copyleft license (GPL, AGPL) was found in the runtime categories.

### Regenerating the Inventory

```sh
# npm: runtime and development packages, with declared licenses
npm query '.prod'
npm query '.dev'

# Rust (run in src-tauri/): runtime, build-time, and dev categories for macOS
cargo tree --locked --target aarch64-apple-darwin -e normal,no-proc-macro --prefix none --format '{p} {l}'
cargo tree --locked --target aarch64-apple-darwin -e normal,build --prefix none --format '{p} {l}'
cargo tree --locked --target aarch64-apple-darwin -e normal,build,dev --prefix none --format '{p} {l}'
```

The build-time set is the second list minus the first. The dev set is the third list minus the second.

## Integrated External Tools

External programs invoked by Text2Create, rather than linked into it, will be listed here with their licenses once an integration is implemented.

| Tool | License | Integration status |
| --- | --- | --- |
| None yet | — | — |
