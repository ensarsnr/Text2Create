# Project Status

**Stage:** Early development — Application Foundation (Milestone 1)

**Last updated:** 2026-11-27

## Summary

The repository contains documentation, policies, repository tooling, and a minimal desktop application skeleton. The skeleton validates the technology direction only. No product features are implemented.

## What Exists

- A Tauri 2 desktop application skeleton for macOS (Apple Silicon) with a React + TypeScript frontend and a Rust core
- One demo command (`greet`) that verifies frontend-to-Rust communication, with input validation and unit tests
- Separate Content Security Policies for production builds and development
- Default-deny command permissions: the main window may only call the `greet` command; no core plugin permissions are granted
- CI checks for the frontend (type-check, build, `npm audit`, registry signature verification) and the desktop app on macOS (formatting, Clippy, tests, release bundle build)
- A Project IR v1 library crate (`src-tauri/crates/text2create-ir`) without a Tauri dependency: data model, strict JSON loading, version checks, invariants, a domain codec interface, canonical serialization, committed fixtures, and a write path that applies typed change sets atomically and creates their provenance. CI verifies that it does not depend on Tauri.

- Project documentation describing vision, scope, and the proposed architecture
- A decision log separating accepted, provisional, and proposed decisions
- A milestone-based roadmap
- Contribution guidelines, a code of conduct, and a security policy
- Issue and pull request templates
- A CI workflow that checks repository structure, Markdown, and YAML
- An empty `tests/` directory reserved for cross-cutting tests

## What Does Not Exist Yet

| Area | Status |
| --- | --- |
| Product features | Not started |
| Project IR | v1 specified ([Project IR](PROJECT_IR.md)); model, strict loading with error locations, invariants, canonical serialization, the write path (#43), and versioned migrations (#11; no format migrations are needed yet, and the application does not migrate projects until the approval flow #26 exists) implemented in the `text2create-ir` crate with tests; persistence (#12) and artifact file storage (#13) implemented in the `text2create-store` crate; persistence is exposed as Tauri commands, which the frontend does not use or have permission for yet. Opening a project reports artifact files changed outside Text2Create; no command writes artifact files yet |
| Local database (SQLite) | One database per project for the Project IR ([ADR-0015](DECISIONS.md#adr-0015-sqlite-library-schema-migrations-and-project-layout)); no task history or settings yet |
| AI provider integrations | Not started |
| Engineering tool integrations (KiCad, ngspice, Arduino CLI, ESP-IDF) | Not started; candidates only |
| Frontend tests | None |
| Releases, code signing, notarization | None |
| Application icons | Tauri template placeholders |

## Current Focus

1. Complete the remaining Milestone 1 validation items (background task with progress events and cancellation, tracked in #25).
2. Collect feedback on the architecture direction, the Project IR concept, and the MVP scope.
3. Milestone 2: artifact file storage (#13) is in review.

## How to Help Now

- Review the [Architecture](ARCHITECTURE.md) and [Project IR](PROJECT_IR.md) documents and open issues with questions or concerns.
- Suggest concrete first workflows for electronics, embedded systems, or robotics.
- Improve documentation clarity.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details.
