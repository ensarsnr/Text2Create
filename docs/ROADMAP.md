# Roadmap

> All milestones below are **planned**. The roadmap is a statement of intent, not a commitment to dates or features. Scope will change as prototypes answer the open questions in the [Decision Log](DECISIONS.md). Current progress is tracked in [Project Status](PROJECT_STATUS.md) and in [GitHub milestones](https://github.com/ensarsnr/Text2Create/milestones).

## Overview

| # | Milestone | Status |
| --- | --- | --- |
| 0 | Repository Foundation | Done |
| 1 | Application Foundation | In progress |
| 2 | Project Core and Project IR | Planned |
| 3 | AI Provider Layer | Planned |
| 4 | Task Orchestration and Execution | Planned |
| 5 | First Engineering Workflow | Planned |
| 6 | Validation and Reliability | Planned |
| 7 | Public Alpha | Planned |

## Milestone 0: Repository Foundation

**Goal:** a clean, honest, public-ready repository that contributors can understand.

- Project documentation: overview, architecture direction, Project IR concept, decision log, roadmap, status
- Contribution guidelines, code of conduct, and security policy
- License and third-party license tracking policy
- Issue and pull request templates, labels, and milestones
- Minimal CI for documentation and repository consistency
- Branch protection for `main` and repository security features

**Done when:** the repository is published with the above in place and CI passes on `main`.

## Milestone 1: Application Foundation

**Goal:** validate the desktop technology direction with a minimal working application.

- Tauri 2 + React + TypeScript application skeleton that builds and runs on macOS
- Rust core with a first Tauri command and a restrictive capability configuration
- SQLite connectivity from the Rust core
- Background task with progress events and cancellation
- Linting, formatting, and unit test setup for Rust and TypeScript
- CI extended to build and test the application
- Decisions on frontend tooling recorded as ADRs

**Done when:** the prototype answers the validation questions in [ADR-0003](DECISIONS.md#adr-0003-local-first-desktop-application-with-tauri-2) and the provisional ADRs are confirmed or revised.

## Milestone 2: Project Core and Project IR

**Goal:** create, open, and persist projects with structured state.

- First Project IR schema, versioned, with migrations
- Project creation, opening, and storage on disk
- Artifact references and basic change history
- Initial rollback approach
- ADR-0008 accepted or revised

**Done when:** a project can be created, modified, saved, reopened, and rolled back through tested core APIs.

## Milestone 3: AI Provider Layer

**Goal:** provider-neutral access to cloud and local models.

- Provider adapter interface
- Adapters for at least one cloud provider and Ollama, with others added incrementally
- Secure API key storage (planned: macOS Keychain)
- Streaming, cancellation, timeouts, and error handling
- Requirement clarification flow (Describe → Understand)

**Done when:** a user can configure a provider and hold a clarification conversation whose outcome is written into the Project IR.

## Milestone 4: Task Orchestration and Execution

**Goal:** structured plans that are executed safely.

- Plan generation into structured, editable task plans (Plan)
- Tool adapter interface with declared permissions
- Approval system for consequential operations
- Execution history and provenance recording
- Pause, resume, cancel, and retry

**Done when:** a reviewed plan can be executed step by step with approvals, and every action is recorded.

## Milestone 5: First Engineering Workflow

**Goal:** one complete, useful workflow in the MVP domains.

- Select one concrete workflow in electronics or embedded systems (for example a simple microcontroller project with firmware build, or a circuit with simulation)
- Integrate only the tools that workflow requires, chosen from the candidates: KiCad, ngspice, Arduino CLI, ESP-IDF
- Run the full workflow: Describe → Understand → Plan → Create → Execute → Verify → Improve

**Done when:** the selected workflow works end to end on macOS for a documented example project.

## Milestone 6: Validation and Reliability

**Goal:** trustworthy results and a robust application.

- Validation engine with domain checks for the first workflow
- Validation results stored and linked to artifact versions
- Improved error handling and recovery
- Expanded automated tests, including integration tests for tool adapters
- Security review of tool execution and prompt-injection handling

**Done when:** validation failures are reliably detected and reported, and known critical issues are resolved.

## Milestone 7: Public Alpha

**Goal:** a first release that early users can install and try.

- Signed and notarized macOS build
- Installation and user documentation
- Clear statement of known limitations
- Process for collecting feedback and bug reports

**Done when:** an alpha release is published with release notes and known limitations.

## Beyond the Alpha

Possible future directions, not yet planned in detail: additional engineering domains (robotics simulation, mechanical CAD, 3D modeling), additional platforms (Windows, Linux), and additional tool integrations.
