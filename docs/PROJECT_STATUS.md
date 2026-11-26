# Project Status

**Stage:** Early development — Foundation stage (Milestone 0)

**Last updated:** 2026-11-26

## Summary

Text2Create is at its initial foundation stage. The repository contains documentation, contribution guidelines, policies, and repository tooling. Development of the application has not started.

## What Exists

- Project documentation describing vision, scope, and the proposed architecture
- A decision log separating accepted, provisional, and proposed decisions
- A milestone-based roadmap
- Contribution guidelines, a code of conduct, and a security policy
- Issue and pull request templates
- A CI workflow that checks repository structure, Markdown, and YAML
- Empty placeholder directories (`src/`, `src-tauri/`, `tests/`) reserved for the planned application layout

## What Does Not Exist Yet

| Area | Status |
| --- | --- |
| Application code (frontend or Rust core) | Not started |
| Project IR schema | Not designed |
| AI provider integrations | Not started |
| Engineering tool integrations (KiCad, ngspice, Arduino CLI, ESP-IDF) | Not started; candidates only |
| Automated application tests | None (no application code to test) |
| Builds, binaries, or releases | None |
| Third-party runtime dependencies | None adopted |

## Current Focus

1. Complete Milestone 0: Repository Foundation.
2. Collect feedback on the architecture direction, the Project IR concept, and the MVP scope.
3. Prepare the Milestone 1 prototype to validate the desktop technology direction.

## How to Help Now

- Review the [Architecture](ARCHITECTURE.md) and [Project IR](PROJECT_IR.md) documents and open issues with questions or concerns.
- Suggest concrete first workflows for electronics, embedded systems, or robotics.
- Improve documentation clarity.

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details.
