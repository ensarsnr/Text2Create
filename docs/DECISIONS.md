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
| [ADR-0008](#adr-0008-structured-project-state-through-a-project-ir) | Structured project state through a Project IR | Accepted (provisional) |
| [ADR-0009](#adr-0009-macos-as-the-initial-target-platform) | macOS as the initial target platform | Accepted |
| [ADR-0010](#adr-0010-monolithic-single-orchestrator-architecture) | Monolithic, single-orchestrator architecture | Accepted |
| [ADR-0011](#adr-0011-explicit-approval-for-consequential-operations) | Explicit approval for consequential operations | Accepted |
| [ADR-0012](#adr-0012-vite-and-npm-for-the-frontend-toolchain) | Vite and npm for the frontend toolchain | Accepted |
| [ADR-0013](#adr-0013-application-bundle-identifier) | Application bundle identifier | Accepted (provisional) |
| [ADR-0014](#adr-0014-project-ir-schema-definition-domain-extensions-and-strict-loading) | Project IR schema definition, domain extensions, and strict loading | Accepted (provisional) |

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

**Open:** state management, component library, and styling approach. The build tool is decided in [ADR-0012](#adr-0012-vite-and-npm-for-the-frontend-toolchain).

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

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26 (proposed), 2026-11-27 (revised and accepted provisionally)

**Context:** Chat history is not a reliable foundation for project state. See [PROJECT_IR.md](PROJECT_IR.md). The original proposal left the storage model, the schema definition approach, change history granularity, and rollback semantics open.

**Decision:** Represent project state as a durable, versioned, structured Project IR that serves as the source of truth for the UI, orchestrator, tools, and validation. Specifically:

- **Logical model:** the IR is a single document (an aggregate of typed records with stable identifiers and references) that is loaded, validated, and checked for invariants as a whole.
- **Persistence is separate:** the logical model does not depend on how it is stored. The physical layout is decided in [#12](https://github.com/ensarsnr/Text2Create/issues/12); SQLite remains the preferred store ([ADR-0006](#adr-0006-sqlite-for-local-persistence)).
- **Records:** project information, requirements, entities, artifacts, links, plans with tasks, validation results, and provenance records. Native tool files stay authoritative for their content; artifacts reference them by path and content digest.
- **Provenance is separate from history:** provenance records in the document record who or what caused each change. Until the change journal exists, they are append-only and never removed. The change journal, which records the operations of each change and enables rollback, is a separate component ([#14](https://github.com/ensarsnr/Text2Create/issues/14)) that references provenance records instead of duplicating them.
- **Planned work is separate from execution:** tasks describe planned work only. Task runs, execution status, and logs are stored outside the IR ([#27](https://github.com/ensarsnr/Text2Create/issues/27)).

The schema definition approach is decided in [ADR-0014](#adr-0014-project-ir-schema-definition-domain-extensions-and-strict-loading).

**Alternatives considered:**

- **Normalized relational model as the logical model** (a table per record type and domain): every domain change becomes a SQL migration, and the expected project sizes do not need relational queries.
- **Generic property graph** (nodes, edges, and property bags): loses type safety, scatters domain structures such as netlists across many edges, and widens the surface for unvalidated model output.
- **Event sourcing with the log as the source of truth:** old events must remain replayable forever, which makes migrations the hardest kind, and files edited outside Text2Create cannot be represented as events.

**Validation required:** the #10 implementation must confirm the model with committed fixtures, byte-identical round trips, and invariant tests, including an illustrative ESP32 project.

**Consequences:** the whole project is held in memory, which is expected to be acceptable for single-user local projects of hundreds to a few thousand records. Because provenance is append-only until the journal exists, the document grows with every change. Provenance retention after the journal exists, change history granularity, and rollback semantics remain open and belong to #14.

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

## ADR-0012: Vite and npm for the frontend toolchain

- **Status:** Accepted
- **Date:** 2026-11-26

**Context:** The application skeleton needs a frontend build tool and a package manager. ADR-0004 left the build tool open.

**Decision:** Use Vite as the frontend dev server and bundler, and npm as the package manager. TypeScript is type-checked with `tsc -b` using project references. Package install scripts are disabled through `.npmrc` (`ignore-scripts=true`).

**Consequences:** Vite is the default choice in the Tauri templates, which keeps the setup close to upstream documentation. npm ships with Node.js, so contributors need no additional package manager. State management, component library, and styling remain open.

---

## ADR-0013: Application bundle identifier

- **Status:** Accepted (provisional)
- **Date:** 2026-11-26

**Context:** Tauri requires a unique reverse-domain bundle identifier. On macOS it becomes the application's permanent identity: it determines the bundle ID, the application data directory, Keychain item scope, code signing, notarization, and update channels.

**Decision:** Use `io.github.ensarsnr.text2create`.

**Consequences:** Changing the identifier after users exist makes macOS treat the application as a different app and requires data and settings migration. The identifier is tied to the current GitHub account; if the project moves to an organization or acquires its own domain, the identifier should be reconsidered **before the Public Alpha (Milestone 7)**, after which it should be treated as final.

---

## ADR-0014: Project IR schema definition, domain extensions, and strict loading

- **Status:** Accepted (provisional)
- **Date:** 2026-11-27

**Context:** [ADR-0008](#adr-0008-structured-project-state-through-a-project-ir) defines what the Project IR contains. The IR must support many domains (electronics, embedded systems, robotics, CAD) without making the shared core complex, and it receives untrusted input from models, files, and tools.

**Decision:**

- **Rust types are the source of truth.** Core records are closed Rust types that reject unknown fields. Any JSON Schema is generated from these types, never maintained by hand. There is no runtime JSON Schema validation.
- **Compiled-in, versioned domain codecs.** Each domain is a Rust codec with its own identifier and version, compiled into the application. Domain payloads are decoded into typed Rust structures, validated, and re-encoded; stored payloads must equal their re-encoded form. Domains report the record identifiers their payloads contain so that the core can enforce referential integrity. Third-party schema loading remains a non-goal ([Architecture](ARCHITECTURE.md#non-goals-for-now)).
- **Strict loading.** Input is parsed with a strict JSON parser that rejects repeated object keys at any depth, non-integer numbers, and out-of-range integers before conversion into JSON values. Version checks run before typed parsing.
- **Unknown data is rejected, not preserved.** Unknown fields, unknown enum values, unavailable domains, and newer format or domain versions cause loading to fail without modifying the file. Older versions are migrated after a backup ([#11](https://github.com/ensarsnr/Text2Create/issues/11)). There is no mechanism that carries unknown data through a save.
- **No floating-point numbers and no core unit type.** Exact non-integer values are canonical decimal strings. Units and their semantics belong to domains.
- **Canonical serialization.** The same state always produces the same bytes, and fixtures are compared byte for byte.

**Alternatives considered:**

- **One closed Rust type tree containing all domains:** strongest compile-time checks, but every domain change becomes a core schema change, and the core accumulates every domain's dependencies.
- **Runtime JSON Schema per domain:** domains could be added without code, but invariants beyond structure still need Rust code, which creates two validation layers, and Rust code loses type safety.
- **Preserving unknown fields on save:** avoids data loss when an older application opens a newer file, but lets unvalidated data persist in the IR. Rejecting the file achieves the same goal of not losing data.

**Validation required:** the #10 implementation must demonstrate the domain interface with a test-only domain and must include negative tests for every rejection rule in [PROJECT_IR.md](PROJECT_IR.md).

**Consequences:** domain payloads pass through an untyped boundary between the core and the domain codec, which must be covered by tests. Opening a project created by a newer application version fails until the application is updated. Adding a domain requires a new application release. Specification details and the decisions taken while specifying v1 are in [PROJECT_IR.md](PROJECT_IR.md#specification-decisions).

---

## Open Questions

These questions do not yet have ADRs. They should be resolved through discussion or prototypes, then recorded as ADRs.

| Topic | Question | Expected milestone |
| --- | --- | --- |
| Frontend tooling | Which state management and component library? (Build tool decided in ADR-0012.) | 1–2 |
| Persistence | Which Rust SQLite library and migration approach? One database per project or global? | 1–2 |
| Project IR history | Change history granularity and rollback semantics (decided in ADR-0008 and ADR-0014: logical model and schema approach) | 2 |
| Artifact versioning | Git, content-addressed storage, or snapshots? | 2 |
| Secrets | macOS Keychain integration approach | 3 |
| Provider interface | How to handle capability differences across providers? | 3 |
| Sandboxing | What isolation is practical for tool execution on macOS? | 4 |
| Approval UX | Per action, per plan, or per session approvals? | 4 |
| First workflow | Which concrete electronics or embedded workflow is the first end-to-end target? | 5 |
| Distribution | Code signing, notarization, and update mechanism for macOS | 7 |
