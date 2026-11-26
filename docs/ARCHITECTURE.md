# Architecture

> **Status:** Proposed. No component described here has been implemented. This document records the current direction so it can be discussed, challenged, and validated. Decisions and their status are tracked in the [Decision Log](DECISIONS.md).

## Status Legend

Each element in this document is labeled with one of the following:

| Label | Meaning |
| --- | --- |
| **Direction** | Current preferred choice. Expected to hold, but still to be confirmed by a working prototype. |
| **Proposal** | A design idea that requires technical validation before adoption. |
| **Open** | A question without a preferred answer yet. |

## Goals

- Run locally on the user's machine, with projects stored locally.
- Keep the user in control of every consequential action.
- Keep project state structured, versioned, and inspectable.
- Allow AI providers and engineering tools to be added without changing the core.
- Stay simple: a single desktop application, not a distributed system.

## Non-Goals (for now)

- Microservices or a server-side backend
- Multi-agent frameworks
- Plugin marketplaces or third-party extension loading
- Multi-user collaboration and cloud sync
- Platforms other than macOS

## High-Level Overview

```text
┌────────────────────────────────────────────────────────────────┐
│                    Tauri 2 desktop application                 │
│                                                                │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │          Frontend: React + TypeScript (webview)          │  │
│  │   conversation · plan review · approvals · artifacts     │  │
│  └───────────────────────────┬──────────────────────────────┘  │
│                              │ Tauri commands / events         │
│  ┌───────────────────────────▼──────────────────────────────┐  │
│  │                Rust application core                     │  │
│  │                                                          │  │
│  │  Task orchestration ──► Tool execution & approvals       │  │
│  │        │                        │                        │  │
│  │        ▼                        ▼                        │  │
│  │  AI provider adapters     Validation engine              │  │
│  │        │                        │                        │  │
│  │        └──────► Project IR ◄────┘                        │  │
│  │                     │                                    │  │
│  │      Artifact management · SQLite persistence            │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────┘
          │                         │                    │
    Cloud AI APIs             Ollama (local)     Engineering tools
                                                 (KiCad, ngspice, …)
```

## Components

### Desktop Shell — Tauri 2 (Direction)

Tauri 2 is the preferred desktop shell. It provides a native window with a system webview, a Rust backend in the same process, and a capability-based permission model for what the frontend may invoke.

Reasons for the preference: small binary size, a Rust core, and an explicit permission model that fits the project's security goals.

To validate: macOS packaging and signing, webview behavior, and the ergonomics of long-running background tasks. See ADR-0003 in the [Decision Log](DECISIONS.md).

### Frontend — React + TypeScript (Direction)

The frontend is responsible for presentation and user interaction only:

- Conversation and requirement clarification
- Plan review and editing
- Approval prompts for consequential operations
- Artifact browsing and preview
- Validation results and project history

The frontend should not hold authoritative project state or call external services directly. It communicates with the Rust core through Tauri commands and events.

**Open:** state management library and component library. The build tooling is Vite with npm ([ADR-0012](DECISIONS.md#adr-0012-vite-and-npm-for-the-frontend-toolchain)).

### Application Core — Rust (Direction)

The Rust core owns project state, orchestration, tool execution, provider access, and persistence. Keeping these responsibilities in Rust centralizes security-relevant logic in one place that the frontend cannot bypass.

**Proposal:** organize the core as a small number of internal Rust modules (or crates in a single workspace) with clear boundaries, rather than as separate services. The exact module split will be decided when implementation starts.

### Local Database — SQLite (Direction)

SQLite is the preferred store for project metadata, Project IR records, task history, provenance, and settings. Large or tool-native artifacts (for example KiCad files or firmware binaries) are expected to remain as regular files in the project directory, referenced from the database.

**Open:**

- Rust database access library
- Migration strategy
- Whether one database per project or one global database (or both) is the better model

### Project IR (Proposal)

The Project IR is the structured, durable representation of a project: its requirements, artifacts, entities, plans, provenance, and validation results. It is the source of truth that the AI, tools, and UI work against, rather than chat history.

The concept is described in [PROJECT_IR.md](PROJECT_IR.md). No schema has been defined yet.

### AI Provider Adapter Layer (Direction, design is a Proposal)

All AI access goes through a provider-neutral interface implemented by adapters:

- Cloud: OpenAI, Anthropic, Google Gemini, and OpenAI-compatible APIs
- Local: Ollama

Expected responsibilities:

- Normalizing requests and responses (messages, tool calls, structured output, streaming)
- Reporting provider capabilities (context size, tool use support, structured output support)
- Handling errors, timeouts, rate limits, and cancellation
- Keeping API keys out of the frontend and out of logs

**Open:** how to handle capability differences between providers, whether to use existing Rust SDKs or direct HTTP clients, and how to evaluate output quality across providers.

### Task Orchestration (Proposal)

The orchestrator turns a user goal into a plan and drives it through the core workflow:

```text
Describe → Understand → Plan → Create → Execute → Verify → Improve
```

Proposed properties:

- Plans are explicit data (stored in the Project IR), not hidden prompt state.
- The user can review, edit, and approve plans before execution.
- Each task has inputs, expected outputs, dependencies, and a status.
- Execution can be paused, resumed, cancelled, and retried.
- A single orchestrator coordinates the work. Multi-agent frameworks are explicitly out of scope for now.

### Tool Execution and Approval System (Proposal)

Engineering tools (for example KiCad, ngspice, Arduino CLI, ESP-IDF) are invoked through tool adapters with declared permissions.

Proposed rules:

- Each tool declares what it needs: file paths, commands, network access, hardware access.
- Operations are classified by impact. Read-only operations inside the project may run automatically; consequential operations require explicit user approval.
- Consequential operations include writing or deleting files outside the project directory, running arbitrary shell commands, network access, flashing or controlling hardware, installing software, and anything that costs money.
- Commands are executed with argument lists, not by concatenating shell strings.
- Approvals and executions are recorded for auditability.

**Open:** sandboxing options on macOS, approval granularity (per action, per plan, per session), and how to present risk clearly to users.

### Validation Engine (Proposal)

The validation engine checks artifacts against requirements and domain rules, and records the results in the Project IR. Examples of possible checks:

- Schema and consistency checks on the Project IR itself
- Electrical rules checks (ERC) and design rules checks (DRC) via KiCad
- Simulation results from ngspice compared against expected ranges
- Firmware compilation via Arduino CLI or ESP-IDF

Validation results feed the Improve stage. A failed validation should never be silently ignored.

### Artifact Management (Proposal)

Artifacts are the concrete outputs of a project: source files, schematics, simulation decks, firmware, reports. Artifact management is expected to:

- Store artifacts in a user-visible project directory using native tool formats where possible
- Track each artifact in the Project IR with type, version, and provenance
- Detect changes made outside Text2Create (for example, a user editing a file in KiCad)
- Support viewing history and restoring previous versions

**Open:** whether to use Git internally for artifact versioning, a content-addressed store, or a simpler snapshot approach.

## Cross-Cutting Concerns

- **Security:** see [SECURITY.md](../SECURITY.md). Secrets stored in the macOS Keychain (Proposal). External content and model output treated as untrusted.
- **Observability:** structured local logs without secrets or sensitive project content by default.
- **Error handling:** failures surfaced to the user with enough context to act on them.
- **Testing:** unit tests for core logic and integration tests for tool adapters, with external tools mocked where they are unavailable in CI.
- **Licensing:** each dependency and integrated tool reviewed for license compatibility. See [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md).

## Validation Plan

Before the architecture direction is confirmed, the following should be prototyped (see [Roadmap](ROADMAP.md), Milestone 1):

1. A minimal Tauri 2 + React + TypeScript application that builds and runs on macOS.
2. A Rust command invoked from the frontend, with a Tauri capability restricting it.
3. SQLite read/write from the Rust core.
4. A long-running background task with progress events and cancellation.

The results will be recorded as updates to the relevant ADRs.
