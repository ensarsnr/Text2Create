# Decision Log

This log records architecture and project decisions using lightweight Architecture Decision Records (ADRs).

## How to Use This Log

- Each ADR has a number, title, status, date, context, decision, and consequences.
- ADRs are never deleted. If a decision changes, a new ADR supersedes the old one and both statuses are updated.
- Significant technical changes should reference an ADR in their pull request.
- To propose a new decision, open an issue with the `architecture` label or submit a pull request adding an ADR with status **Proposed**.

### Status Values

| Status | Meaning |
| --- | --- |
| **Proposed** | Under discussion. Not yet agreed. |
| **Accepted** | Agreed and in effect. |
| **Accepted (provisional)** | Preferred direction, adopted for planning, but must be confirmed by a prototype or evidence before it is considered final. |
| **Superseded** | Replaced by a later ADR. |
| **Rejected** | Considered and declined. |

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [ADR-0001](#adr-0001-record-decisions-in-a-decision-log) | Record decisions in a decision log | Accepted |
| [ADR-0002](#adr-0002-license-under-apache-20) | License under Apache 2.0 | Accepted |
| [ADR-0003](#adr-0003-local-first-desktop-application-with-tauri-2) | Local-first desktop application with Tauri 2 | Accepted (provisional) |
| [ADR-0004](#adr-0004-react-and-typescript-for-the-frontend) | React and TypeScript for the frontend | Accepted (provisional) |
| [ADR-0005](#adr-0005-rust-for-the-application-core) | Rust for the application core | Accepted (provisional) |
| [ADR-0006](#adr-0006-sqlite-for-local-persistence) | SQLite for local persistence | Accepted (provisional) |
| [ADR-0007](#adr-0007-hybrid-ai-provider-adapter-layer) | Hybrid AI provider adapter layer | Accepted (provisional) |
| [ADR-0008](#adr-0008-structured-project-state-through-a-project-ir) | Structured project state through a Project IR | Proposed |
| [ADR-0009](#adr-0009-macos-as-the-initial-target-platform) | macOS as the initial target platform | Accepted |
| [ADR-0010](#adr-0010-monolithic-single-orchestrator-architecture) | Monolithic, single-orchestrator architecture | Accepted |
| [ADR-0011](#adr-0011-explicit-approval-for-consequential-operations) | Explicit approval for consequential operations | Accepted |

---

## ADR-0001: Record decisions in a decision log

- **Status:** Accepted
- **Date:** 2026-11-26

**Context:** The project is at an early stage and many technical choices are preferences rather than validated decisions. Contributors need to know which choices are settled and which are open.

**Decision:** Record decisions as ADRs in this file. Distinguish accepted, provisional, and proposed decisions explicitly.

**Consequences:** Technology preferences are visible and open to challenge. If the log grows large, ADRs may be moved into individual files under `docs/adr/`.

---

## ADR-0002: License under Apache 2.0

- **Status:** Accepted
- **Date:** 2026-11-26

**Context:** The project is intended to be open source, to welcome contributions, and to be usable in both personal and commercial settings.

**Decision:** License the project under the Apache License 2.0.

**Consequences:** Contributions are accepted under Apache 2.0 terms. The explicit patent grant provides clarity for users and contributors. Dependencies and integrated tools must have compatible licenses. Integration with copyleft-licensed tools (for example GPL-licensed engineering tools) must be designed carefully, for example by invoking them as separate programs rather than linking them. See [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md).

---

## ADR-0003: Local-first desktop application with Tauri 2

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26

**Context:** Text2Create needs to operate on local files, run local engineering tools, access local hardware, and optionally use local AI models. A web-only application cannot do this well.

**Decision:** Build a local-first desktop application. Tauri 2 is the preferred framework.

**Alternatives considered:** Electron (larger footprint, JavaScript backend), a native Swift application (macOS only, smaller contributor pool), a local web server with a browser UI (weaker integration and security model).

**Validation required:** A Milestone 1 prototype must confirm macOS build and packaging, Rust–frontend communication, Tauri capability configuration, and long-running background tasks.

**Consequences:** Contributors need both Rust and TypeScript toolchains. The webview differs between platforms, which matters if other platforms are added later.

---

## ADR-0004: React and TypeScript for the frontend

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26

**Context:** The UI needs rich interaction: conversations, plan editors, approval dialogs, and artifact previews.

**Decision:** Use React with TypeScript.

**Open:** build tool, state management, component library, and styling approach. These will be decided in Milestone 1.

**Consequences:** A large ecosystem and contributor pool. Frontend dependencies must be kept deliberate to limit supply-chain risk.

---

## ADR-0005: Rust for the application core

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26

**Context:** The core handles project state, process execution, file access, and secrets. It must be reliable and secure.

**Decision:** Implement the application core in Rust.

**Consequences:** Memory safety and strong typing for security-relevant code. Natural fit with Tauri. A steeper learning curve for some contributors.

---

## ADR-0006: SQLite for local persistence

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26

**Context:** Projects need durable local storage for structured state, history, and settings, without requiring users to run a database server.

**Decision:** Use SQLite as the local database.

**Open:** database access library, migration tooling, and per-project versus global database layout.

**Consequences:** A single-file, embedded, widely supported store. Large tool-native artifacts remain as files in the project directory.

---

## ADR-0007: Hybrid AI provider adapter layer

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26

**Context:** Users differ in cost, privacy, and capability needs. Dependence on a single AI provider is a risk.

**Decision:** Access AI models through a provider-neutral adapter interface supporting cloud providers (OpenAI, Anthropic, Google Gemini, and OpenAI-compatible APIs) and local models via Ollama.

**Open:** the interface shape, handling of capability differences (tool calling, structured output, context size), and whether to use provider SDKs or direct HTTP clients.

**Consequences:** More work than a single integration, but no lock-in. The core must not depend on provider-specific behavior.

---

## ADR-0008: Structured project state through a Project IR

- **Status:** Proposed
- **Date:** 2026-11-26

**Context:** Chat history is not a reliable foundation for project state. See [PROJECT_IR.md](PROJECT_IR.md).

**Proposal:** Represent project state as a durable, versioned, structured Project IR that serves as the source of truth for the UI, orchestrator, tools, and validation.

**Open:** storage model (relational, document, or hybrid), schema definition approach, change history granularity, and rollback semantics. To be resolved in Milestone 2.

---

## ADR-0009: macOS as the initial target platform

- **Status:** Accepted
- **Date:** 2026-11-26

**Context:** Supporting several operating systems from the start would slow down core development.

**Decision:** Target macOS first. Avoid unnecessary platform-specific assumptions so that Windows and Linux support can be evaluated later.

**Consequences:** CI for application code will need macOS runners. Some engineering tools behave differently across platforms, which must be considered before expanding.

---

## ADR-0010: Monolithic, single-orchestrator architecture

- **Status:** Accepted
- **Date:** 2026-11-26

**Context:** Distributed architectures and multi-agent frameworks add significant complexity before the core workflow has been proven.

**Decision:** Build a single desktop application with a single task orchestrator. Do not introduce microservices or multi-agent frameworks at this stage.

**Consequences:** Simpler development, debugging, and security review. This decision may be revisited later with evidence of a concrete need.

---

## ADR-0011: Explicit approval for consequential operations

- **Status:** Accepted
- **Date:** 2026-11-26

**Context:** Text2Create is intended to execute operations on the user's machine based on AI-generated plans. AI output can be wrong or manipulated through prompt injection.

**Decision:** Consequential operations require explicit user approval. Tools operate with least privilege. The exact classification of operations and approval granularity will be defined during Milestone 4.

**Consequences:** Some friction for users in exchange for safety and trust. The approval system is a core component, not an optional feature.

---

## Open Questions

These questions do not yet have ADRs. They should be resolved through discussion or prototypes, then recorded as ADRs.

| Topic | Question | Expected milestone |
| --- | --- | --- |
| Frontend tooling | Which build tool, state management, and component library? | 1 |
| Persistence | Which Rust SQLite library and migration approach? One database per project or global? | 1–2 |
| Project IR | Relational, document, or hybrid model? How are domain types defined? | 2 |
| Artifact versioning | Git, content-addressed storage, or snapshots? | 2 |
| Secrets | macOS Keychain integration approach | 3 |
| Provider interface | How to handle capability differences across providers? | 3 |
| Sandboxing | What isolation is practical for tool execution on macOS? | 4 |
| Approval UX | Per action, per plan, or per session approvals? | 4 |
| First workflow | Which concrete electronics or embedded workflow is the first end-to-end target? | 5 |
| Distribution | Code signing, notarization, and update mechanism for macOS | 7 |
