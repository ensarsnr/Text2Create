# Project Overview

> This document describes intent and direction. Text2Create is in its foundation stage and no application functionality exists yet. See [Project Status](docs/PROJECT_STATUS.md).

## What Text2Create Is

Text2Create is an open-source, AI-powered creation and execution platform. A user describes what they want to build in natural language, and the platform helps turn that description into concrete results: structured plans, project files, designs, simulations, firmware, and other artifacts, produced and checked through real engineering tools.

Text2Create is **not** intended to be a chatbot. Conversation is one input to the system, but the primary outputs are durable project artifacts and a structured record of how they were produced.

## The Problem

Turning an idea into a working technical project usually involves:

- Translating vague goals into precise requirements
- Choosing appropriate tools, components, and methods
- Producing many interdependent files across different tools
- Running those tools correctly, often with steep learning curves
- Verifying that the result actually meets the requirements
- Iterating without losing track of what changed and why

General-purpose AI assistants can help with individual steps, but their output typically lives in chat history: unstructured, hard to verify, and disconnected from the tools that produce real artifacts. Text2Create aims to connect intent, planning, tool execution, and verification in one coherent, auditable workflow.

## Intended Users

- **Engineers and developers** who want to accelerate routine parts of technical projects while keeping control of decisions.
- **Makers, hobbyists, and students** working on electronics, embedded systems, and robotics who need guidance across unfamiliar tools.
- **Researchers and educators** who need reproducible, inspectable project workflows.
- **Contributors** interested in building tool integrations for specific engineering domains.

## Core Workflow

```text
Describe → Understand → Plan → Create → Execute → Verify → Improve
```

1. **Describe:** the user states a goal in natural language.
2. **Understand:** the system interprets the goal and asks clarifying questions instead of guessing at ambiguous requirements.
3. **Plan:** the system produces a structured task plan that the user can review and edit.
4. **Create:** the system produces artifacts such as source files, schematics, configuration, and documentation.
5. **Execute:** the system runs tools to build, simulate, or deploy, requesting approval for consequential operations.
6. **Verify:** the system validates results against requirements using appropriate checks and tools.
7. **Improve:** the system iterates based on validation results and user feedback, keeping a history of changes.

## Long-Term Vision

Over time, Text2Create aims to support a wide range of technical and creative disciplines, for example:

- Software development
- Electronics and circuit design
- Embedded systems
- Robotics and mechatronics
- Mechanical engineering
- CAD and 3D modeling
- Simulation and testing
- Scientific research
- Automation and digital creation

Each domain is expected to be added through well-defined tool integrations and validation rules built on a shared core, rather than as separate products.

## MVP Scope

The first MVP focuses on a narrow set of related domains so that the core can be validated end to end:

- Electronics
- Embedded systems
- Robotics

Candidate integrations being evaluated (none are implemented):

- **KiCad** for schematic and PCB design
- **ngspice** for circuit simulation
- **Arduino CLI** for firmware build and upload
- **ESP-IDF** for ESP32 firmware development

The exact MVP workflow will be defined in [Milestone 5](docs/ROADMAP.md) after the core has been built.

## Project Principles

- **Honesty over appearance.** Documentation describes what exists. Planned features are labeled as planned.
- **User control.** The user reviews plans and approves consequential operations. The system does not silently act on the user's behalf.
- **Structured state over chat history.** Project knowledge lives in a durable, versioned structure ([Project IR](docs/PROJECT_IR.md)), not only in conversations.
- **Verification is part of the workflow.** Generated output is checked, and validation results are recorded.
- **Local-first.** Projects are stored on the user's machine. Local AI models are supported alongside cloud providers.
- **Provider independence.** No single AI provider is required.
- **Security by default.** Least privilege, explicit approvals, and treating external content as untrusted.
- **Incremental, validated architecture.** Technology choices are validated with prototypes before they are treated as final.
- **Simplicity first.** No microservices, multi-agent frameworks, or speculative abstractions until a concrete need is demonstrated.

## Explicit Limitations

- No application code exists yet. Nothing described here can be used today.
- AI-generated output can be wrong. Text2Create is intended to assist, not to replace, engineering judgment. Users remain responsible for reviewing designs, especially for anything involving safety, mains voltage, batteries, moving machinery, or regulatory compliance.
- Planned integrations depend on third-party tools with their own licenses, platform support, and limitations. Some may prove impractical and be dropped.
- The initial target platform is macOS. Other platforms are not currently planned for the first releases.
- Cloud AI providers require the user's own API keys and are subject to those providers' terms, pricing, and data policies.
- Local models via Ollama may be less capable than cloud models for complex tasks.
