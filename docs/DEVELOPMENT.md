# Development Guide

This guide explains how to work with the repository today and outlines the toolchain expected once application development begins.

## Current State

The repository contains documentation and repository tooling only. There is no application to build or run yet.

## Repository Layout

```text
.
├── .github/              Issue and PR templates, CI workflow, Dependabot configuration
├── docs/                 Architecture, decisions, roadmap, status, and guides
├── scripts/              Repository maintenance scripts
├── src/                  Reserved for the React + TypeScript frontend (empty)
├── src-tauri/            Reserved for the Tauri 2 / Rust application core (empty)
├── tests/                Reserved for cross-cutting tests (empty)
├── CONTRIBUTING.md
├── PROJECT_OVERVIEW.md
├── README.md
├── SECURITY.md
└── THIRD_PARTY_LICENSES.md
```

The `src/` and `src-tauri/` names follow the default Tauri project layout. They contain only a `.gitkeep` file until Milestone 1.

## Prerequisites (current)

To run the repository checks locally you need:

- Git
- Python 3.10 or later (for the repository structure check)
- Node.js 20 or later (optional, for Markdown linting via `npx`)
- [yamllint](https://yamllint.readthedocs.io/) (optional, for YAML linting)

## Running Checks Locally

CI runs the same checks on every pull request and on pushes to `main`.

Repository structure and internal link check:

```sh
python3 scripts/check_repository.py
```

Markdown lint, using the configuration in `.markdownlint-cli2.jsonc`:

```sh
npx markdownlint-cli2 "**/*.md"
```

YAML lint, using the configuration in `.yamllint.yml`:

```sh
yamllint .
```

## Planned Toolchain (Milestone 1)

The following toolchain is expected once application development begins. It is not required yet and will be confirmed in Milestone 1.

- Rust (stable) via `rustup`
- Node.js (LTS) and a package manager (to be decided)
- Xcode Command Line Tools (macOS)
- Tauri 2 prerequisites for macOS

Build, test, and lint commands will be documented here when they exist.

## Workflow

1. Create a branch from `main` using the naming conventions in [CONTRIBUTING.md](../CONTRIBUTING.md#branches).
2. Make focused changes and run the checks above.
3. Commit using [Conventional Commits](https://www.conventionalcommits.org/).
4. Open a pull request. CI must pass before merging.
5. Pull requests are merged with squash merge.
