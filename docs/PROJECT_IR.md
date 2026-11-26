# Project IR

> **Status:** Concept. No schema has been designed or implemented. This document explains the idea and the requirements it should satisfy so that a schema can be designed in [Milestone 2](ROADMAP.md).

## What the Project IR Is

The Project IR (Project Intermediate Representation) is the durable, structured state of a Text2Create project. It records what the project is, what it contains, how its parts relate to each other, what work is planned or done, and how each result was produced and verified.

It is the shared source of truth for the user interface, the task orchestrator, the AI provider layer, the tool adapters, and the validation engine.

## Why Not Chat History

Chat history is a poor foundation for real projects:

- It is unstructured, so tools cannot reliably read or validate it.
- It grows without bound and eventually exceeds model context limits.
- Decisions and facts are scattered and can be contradicted later in the conversation.
- It does not record which artifact came from which request, model, or tool.
- It cannot be safely rolled back to a known-good state.

The Project IR keeps conversation as an input and records its outcomes as structured data. A model can be given a focused, relevant slice of the IR instead of an entire conversation.

## Potential Elements

The following elements are candidates for the IR. They are not a schema, and the final design may merge, split, or drop some of them.

| Element | Purpose |
| --- | --- |
| Project metadata | Name, description, domain, creation date, schema version, settings |
| Requirements | User goals and constraints, including clarifications and their sources |
| Artifacts | References to files and generated outputs, with type, location, and version |
| Entities | Domain objects such as components, nets, modules, parts, or functions |
| Relationships | Typed links between entities and artifacts, for example "component U1 is placed in schematic S" |
| Task plans | Structured plans with tasks, status, inputs, and expected outputs |
| Dependencies | Ordering and data dependencies between tasks and artifacts |
| Provenance | Which request, plan, model, prompt, tool, and approval produced each change |
| Validation results | Checks performed, their outcomes, and the artifact versions they apply to |
| Schema versions | Version of the IR schema used by the project, to support migrations |
| Change history | An ordered record of changes to the IR |
| Rollback support | The ability to restore the IR and related artifacts to an earlier state |

## Design Requirements

Whatever schema is eventually chosen should satisfy these requirements:

1. **Durable:** stored locally and survives application restarts and upgrades.
2. **Versioned schema:** every project records its schema version, and migrations are explicit and tested.
3. **Domain-extensible:** new domains (for example mechanical CAD) can add entity and artifact types without changing the core model.
4. **Traceable:** every change can be traced to its cause (a user action, a plan step, a tool run).
5. **Verifiable:** validation results are attached to specific artifact versions, so stale results can be detected.
6. **Reversible:** changes can be undone or rolled back to a known state.
7. **Inspectable:** users can see and understand what the IR contains. Nothing important is hidden in opaque blobs.
8. **Tool-friendly:** native tool files (for example KiCad projects) remain the authoritative format for their content. The IR references and summarizes them rather than replacing them.
9. **Model-friendly:** relevant parts of the IR can be serialized into compact, structured context for AI models.
10. **Safe against untrusted input:** content originating from models, files, or external sources is validated before it is written into the IR.

## Open Questions

- Should the IR be primarily relational (SQLite tables), document-based (JSON documents stored in SQLite), or a hybrid?
- How should domain-specific entity types be defined and validated (for example JSON Schema, Rust types, or both)?
- How fine-grained should change history be: per field, per entity, or per operation?
- How should rollback interact with external files and with actions that cannot be reversed (for example flashing firmware to a device)?
- How should the IR detect and reconcile changes made to artifacts outside Text2Create?
- What is the minimum IR needed for the first engineering workflow in Milestone 5?

These questions are tracked in the [Decision Log](DECISIONS.md) and should be answered with small prototypes before a schema is finalized.
