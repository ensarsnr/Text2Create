# Text2Create

Text2Create is an open-source, AI-powered creation and execution platform. Its goal is to turn natural-language instructions into real projects, creations, and executable tasks.

> [!IMPORTANT]
> **Project status: Early development — Foundation stage.**
> This repository currently contains project documentation, contribution guidelines, and repository tooling only. **No application code has been written yet.** Every capability described below is planned, not implemented. See [Project Status](docs/PROJECT_STATUS.md) for details.

## Installation

There is **no downloadable release yet**. Text2Create cannot be installed or used at this stage.

When releases become available, they will be published on the [Releases page](https://github.com/ensarsnr/Text2Create/releases) together with release notes and known limitations. Only download builds from that page. Release tags are protected and releases are immutable once published. Progress toward the first release is tracked in the [Roadmap](docs/ROADMAP.md).

## Vision and Purpose

Many technical projects start as an idea expressed in plain language and then require a long chain of manual work: clarifying requirements, choosing tools, producing files, running tools, and checking that the results are correct.

Text2Create aims to support that whole chain, not just the conversation. It is intended to be a multidisciplinary AI production platform rather than a chatbot: a tool that understands what a user wants to build, produces a structured plan, creates real project artifacts, runs authorized operations, and verifies the outcome.

Read the full [Project Overview](PROJECT_OVERVIEW.md) for the problem statement, intended users, and principles.

## Core Workflow

```text
Describe → Understand → Plan → Create → Execute → Verify → Improve
```

| Stage | Intent |
| --- | --- |
| Describe | The user explains what they want in natural language. |
| Understand | The system interprets the request and asks clarifying questions where requirements are ambiguous. |
| Plan | The system produces a structured, reviewable plan of tasks. |
| Create | The system generates project artifacts such as files, designs, and configurations. |
| Execute | The system runs operations through tools, with user approval for consequential actions. |
| Verify | The system checks results against requirements using validation tools. |
| Improve | The system iterates on the results based on validation feedback and user input. |

## Planned Capabilities

The following capabilities are **planned**. None of them are available today.

- Requirement clarification through guided questions
- Structured, user-reviewable task plans
- A durable structured project representation ([Project IR](docs/PROJECT_IR.md)) instead of relying on chat history alone
- Pluggable AI providers, both cloud-hosted and local
- Tool execution with explicit permission and approval controls
- Validation of generated artifacts using domain tools
- Provenance tracking: which inputs, models, and tools produced each artifact
- Iterative refinement with change history and rollback

## Initial MVP Scope

The long-term vision covers many technical and creative disciplines. The first MVP is intentionally narrow and focuses on:

- **Electronics**
- **Embedded systems**
- **Robotics**

Candidate engineering integrations under evaluation for the MVP:

| Tool | Domain | Status |
| --- | --- | --- |
| [KiCad](https://www.kicad.org/) | Schematic and PCB design | Candidate — not integrated |
| [ngspice](https://ngspice.sourceforge.io/) | Circuit simulation | Candidate — not integrated |
| [Arduino CLI](https://arduino.github.io/arduino-cli/) | Firmware build and upload | Candidate — not integrated |
| [ESP-IDF](https://docs.espressif.com/projects/esp-idf/) | ESP32 firmware development | Candidate — not integrated |

Inclusion in this table does not mean an integration exists or is guaranteed. See the [Roadmap](docs/ROADMAP.md).

## Architecture Direction

The current preferred direction is a local-first desktop application:

- **Desktop shell:** Tauri 2
- **Frontend:** React + TypeScript
- **Application core:** Rust
- **Local storage:** SQLite
- **Project state:** Project IR (structured intermediate representation)
- **AI integration:** a provider adapter layer supporting cloud and local models

This is a direction, not a finalized design. Several elements still require technical validation. See [Architecture](docs/ARCHITECTURE.md) and the [Decision Log](docs/DECISIONS.md).

## Planned Technology Stack

| Area | Planned choice | Notes |
| --- | --- | --- |
| Desktop framework | Tauri 2 | Preferred; to be validated by a prototype |
| UI | React, TypeScript | Preferred |
| Core logic | Rust | Preferred |
| Persistence | SQLite | Preferred for local project data |
| Cloud AI | OpenAI, Anthropic, Google Gemini, and compatible APIs | Via provider adapters |
| Local AI | Ollama | Via provider adapter |
| Initial platform | macOS | Other platforms may follow later |

## Security Principles

Text2Create is intended to run AI-driven operations on a user's machine, so security is a design requirement from the start:

- **Least privilege:** tools and providers receive only the access they need.
- **Explicit approval:** consequential operations (writing outside the project, running commands, flashing hardware, network access, spending money) require user approval.
- **No secrets in the repository:** API keys and credentials must never be committed. Runtime secrets are intended to be stored in the operating system's secure storage.
- **Untrusted input:** content from files, web pages, tool output, and model responses is treated as untrusted data, including against prompt injection.
- **Auditability:** actions should be traceable to the request, plan, and approval that caused them.

See [SECURITY.md](SECURITY.md) for vulnerability reporting and the full security policy.

## Documentation

| Document | Description |
| --- | --- |
| [Project Overview](PROJECT_OVERVIEW.md) | What Text2Create is, the problem it addresses, and its principles |
| [Architecture](docs/ARCHITECTURE.md) | Proposed high-level architecture |
| [Project IR](docs/PROJECT_IR.md) | The structured project representation concept |
| [Decision Log](docs/DECISIONS.md) | Architecture decisions and open questions |
| [Roadmap](docs/ROADMAP.md) | Planned milestones |
| [Project Status](docs/PROJECT_STATUS.md) | What exists today and what does not |
| [Development Guide](docs/DEVELOPMENT.md) | Local setup and repository checks |
| [Contributing](CONTRIBUTING.md) | How to contribute |
| [Security Policy](SECURITY.md) | Vulnerability reporting and security expectations |
| [Third-Party Licenses](THIRD_PARTY_LICENSES.md) | How dependencies and licenses are tracked |

## Contributing

Contributions are welcome, and at this stage the most valuable contributions are feedback on the architecture, the Project IR concept, and the MVP scope. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md) before opening an issue or pull request. For questions and ideas, use [GitHub Discussions](https://github.com/ensarsnr/Text2Create/discussions).

## License

Text2Create is licensed under the [Apache License 2.0](LICENSE).
