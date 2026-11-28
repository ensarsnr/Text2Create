# Project IR

> **Status:** v1 specification — **draft, implementation in progress.** This document specifies the first version of the Project IR data model. The model, strict loading with [error locations](#error-locations), invariants, canonical serialization, the [write path](#write-path), and [migrations](#migrations) are implemented in `src-tauri/crates/text2create-ir`; projects are persisted by `text2create-store` (ADR-0015). Implementation is tracked in [#10](https://github.com/ensarsnr/Text2Create/issues/10). The underlying decisions are [ADR-0008](DECISIONS.md#adr-0008-structured-project-state-through-a-project-ir) and [ADR-0014](DECISIONS.md#adr-0014-project-ir-schema-definition-domain-extensions-and-strict-loading), both **Accepted (provisional)** until the implementation confirms them. Decisions taken while specifying v1 are listed under [Specification Decisions](#specification-decisions).

## What the Project IR Is

The Project IR (Project Intermediate Representation) is the durable, structured state of a Text2Create project. It records what the project is, what it contains, how its parts relate to each other, what work is planned, and how each result was produced and verified.

It is the shared source of truth for the user interface, the task orchestrator, the AI provider layer, the tool adapters, and the validation engine.

## Why Not Chat History

Chat history is a poor foundation for real projects:

- It is unstructured, so tools cannot reliably read or validate it.
- It grows without bound and eventually exceeds model context limits.
- Decisions and facts are scattered and can be contradicted later in the conversation.
- It does not record which artifact came from which request, model, or tool.
- It cannot be safely rolled back to a known-good state.

The Project IR keeps conversation as an input and records its outcomes as structured data. A model can be given a focused, relevant slice of the IR instead of an entire conversation.

## Design Requirements

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

At the model level, v1 addresses requirements 2, 3, 5, 7, 8, and 10. Requirement 1 is addressed by persistence outside the IR crate ([#12](https://github.com/ensarsnr/Text2Create/issues/12), ADR-0015). Requirement 4 is addressed by append-only provenance records (see [Provenance](#provenance)): every change is attributed to its cause. Recording the operations of each change and requirement 6 are specified in [ADR-0018](DECISIONS.md#adr-0018-project-history-and-rollback) ([#14](https://github.com/ensarsnr/Text2Create/issues/14)) and not implemented yet. Requirement 9 is addressed by a separate context projection in Milestone 3.

## Principles

- **Logical model independent of storage.** The IR is a single logical document that is loaded, validated, and reasoned about as a whole. How it is stored is decided separately: [ADR-0015](DECISIONS.md#adr-0015-sqlite-library-schema-migrations-and-project-layout) stores the canonical document in a per-project SQLite database ([#12](https://github.com/ensarsnr/Text2Create/issues/12)). The model contains no storage concepts such as row IDs.
- **Rust types are the source of truth.** The schema is defined by Rust types in a separate crate of the application core that does not depend on Tauri. A JSON Schema, if needed (for example for model structured output or frontend types), is generated from them, never maintained by hand.
- **Compiled-in, versioned domains.** Domain-specific content is defined by domain codecs compiled into the application. There is no runtime loading of third-party schemas.
- **Strict loading.** Data that cannot be fully validated is rejected, never partially accepted, and never written back.
- **No floating-point numbers.** Numbers in the IR are integers. Exact non-integer values are decimal strings.
- **Planned work is not execution.** The IR stores plans and tasks. Execution attempts (task runs) and logs are stored outside the IR ([#27](https://github.com/ensarsnr/Text2Create/issues/27)).

## Scope of v1

| Included in v1 | Not in v1 | Tracked in |
| --- | --- | --- |
| Project information, requirements, entities, artifacts, links, plans and tasks, validation results, provenance records | — | [#10](https://github.com/ensarsnr/Text2Create/issues/10) |
| Typed domain extension interface and a test-only example domain | Electronics domain schema | [#30](https://github.com/ensarsnr/Text2Create/issues/30) |
| — | Embedded (firmware) domain schema | [#44](https://github.com/ensarsnr/Text2Create/issues/44) |
| Strict loading, version checks, canonical serialization, invariants, error locations, migrations between versions (see [Migrations](#migrations)) | Format migrations: v1 is the first format version, so none exist yet | [#11](https://github.com/ensarsnr/Text2Create/issues/11) |
| Write path: change sets, operations, provenance creation (see [Write Path](#write-path)) | — | [#43](https://github.com/ensarsnr/Text2Create/issues/43); required before #12 |
| — | Persistence: implemented outside the IR crate by `text2create-store` ([ADR-0015](DECISIONS.md#adr-0015-sqlite-library-schema-migrations-and-project-layout)) | [#12](https://github.com/ensarsnr/Text2Create/issues/12) |
| `Digest` value format | Computing content digests, file access, detecting external file changes | [#13](https://github.com/ensarsnr/Text2Create/issues/13) |
| — | Change history and rollback (see [History and Rollback](#history-and-rollback)) | [#14](https://github.com/ensarsnr/Text2Create/issues/14) |
| `TaskRunId` as a reference type | Task runs, execution status, logs, timeouts | [#25](https://github.com/ensarsnr/Text2Create/issues/25), [#27](https://github.com/ensarsnr/Text2Create/issues/27) |
| User-only decision fields (see [Invariants](#invariants)) | Approval workflow and approval records | [#26](https://github.com/ensarsnr/Text2Create/issues/26) |

## Document Structure

A Project IR document is a JSON object with exactly these fields, in this order:

| Field | Type | Description |
| --- | --- | --- |
| `format` | object | `{ "schema": "text2create.project-ir", "version": 1 }` |
| `domains` | array | Domains used by the project: `{ "id", "version" }`, sorted by `id`. The built-in `core` domain is implied by `format` and not listed. |
| `project` | object | Project information |
| `provenance` | array | Provenance records |
| `requirements` | array | Requirements |
| `entities` | array | Domain entities |
| `artifacts` | array | Artifact references |
| `links` | array | Typed links between records |
| `plans` | array | Plans, each containing its tasks |
| `validations` | array | Validation results |

Every record collection is an array sorted by record ID. Arrays are used instead of objects keyed by ID because a JSON object with a repeated key would otherwise silently keep only the last value.

## Identifiers

- Every record has a stable identifier: a type prefix, an underscore, and a UUID in lowercase hyphenated form, for example `req_01a21c90-0000-7000-8000-000000000011`.
- New identifiers are UUIDv7 (time-ordered, RFC 9562). Loading accepts any UUID in canonical form.
- Identifiers are allocated by the application core. Models and the frontend never choose identifiers.
- Each record type has its own Rust type, so an artifact ID cannot be used where a requirement ID is expected.

| Prefix | Record |
| --- | --- |
| `prj` | Project |
| `prv` | Provenance record |
| `req` | Requirement |
| `ent` | Entity |
| `art` | Artifact |
| `lnk` | Link |
| `pln` | Plan |
| `tsk` | Task |
| `val` | Validation result |
| `run` | Task run (reference only; stored outside the IR) |

A record reference (`RecordRef`) is serialized as the prefixed identifier; the prefix determines the record type.

## Scalar Types

Scalar values are validated by rejecting invalid input, never by silently normalizing it. This keeps loading and saving exact.

Text and paths are not Unicode-normalized in v1: NFC is neither required nor applied automatically. Strings that look identical but use different code points are therefore different values.

| Type | Format |
| --- | --- |
| `Text<N>` | Non-empty, at most `N` characters, no leading or trailing whitespace, no control characters except line feed |
| `DomainId` | `[a-z][a-z0-9_]{0,31}`; `core` is reserved |
| `Kind` | `<domain>.<name>` with one or more `[a-z][a-z0-9_]*` name segments separated by dots, for example `embedded.arduino_cli.compile`; at most 128 bytes |
| `ProjectPath` | Relative path with `/` separators, at most 1024 bytes and 255 bytes per component; rejects absolute paths, empty, `.` and `..` components, backslashes, NUL, and control characters. Filesystem checks (symbolic links, canonicalization) belong to #13. |
| `Digest` | `sha256:` followed by 64 lowercase hexadecimal digits |
| `Timestamp` | UTC with millisecond precision: `YYYY-MM-DDTHH:MM:SS.mmmZ` |
| `Decimal` | Canonical decimal string, for example `28` or `-0.5`; rejects `28.0`, `028`, `-0`, and exponents. A shared library type for domain payloads; not used by core records in v1. |

## Records

### Project Information

`project`: `id`, `provenance`, `name` (`Text<120>`), `description` (`Text<2000>` or `null`).

### Provenance

Provenance records record who or what caused each change to the project. They are **append-only**: a provenance record is never changed or removed, even when no current record references it anymore, for example after a record was modified again or removed, or after a rollback. Provenance records are kept indefinitely, because restored records reference them. Provenance is separate from the change history ([History and Rollback](#history-and-rollback)), which records the operations of each change and references provenance records by ID instead of duplicating them.

Because the document grows with every change, long-lived projects may approach the document size limit. This is accepted for v1.

A provenance record has:

| Field | Description |
| --- | --- |
| `id` | `prv_…` |
| `at` | `Timestamp` |
| `actor` | `user`, `model` (`provider`, `model`), `tool` (`name`, `version`), or `system` (`component`) |
| `activity` | `user_edit`, `interpretation` (a model turned user input into structured records), `task_run` (`task`, `run`), `import`, or `migration` (`from`, `to`) |
| `inputs` | Records the change was derived from: `{ "record", "rev", "digest" }`, pinned to a revision or content digest where known |
| `model` | For model involvement: digests of the prompt and the response, or `null` |

Mutable records carry `provenance: { "rev", "created", "modified" }`. `rev` starts at 1 and increases with each modification. `created` and `modified` reference provenance records. Decisions (for example a confirmed requirement) carry their own `by` reference. Immutable records (validation results, artifact content records) reference a single provenance record.

References from provenance records (`inputs`, and the task of a `task_run` activity) are historical. They may point to records that have since been removed.

The actor is assigned by the trusted Rust layer that knows where a change comes from. It is never taken from frontend or model input. Provenance records are created only by the write path ([#43](https://github.com/ensarsnr/Text2Create/issues/43)).

### Requirements

| Field | Description |
| --- | --- |
| `kind` | `functional`, `performance`, `constraint`, `interface`, `safety` |
| `priority` | `must`, `should`, `could` |
| `status` | `proposed`; `confirmed` (`by`); `rejected` (`by`); `superseded` (`replacement`, `by`) |
| `title`, `statement` | `Text<120>`, `Text<2000>` |
| `origin` | `user` (`quote`); `clarification` (`question`, `answer`); `inferred` (`rationale`) |
| `acceptance` | Acceptance criteria, `Text<500>` each |

Requirements inferred by a model are `proposed` until the user confirms them. Only confirmed requirements can be addressed by tasks.

The core has no quantity or unit type. Requirement statements are human-readable text. Machine-checkable values (for example a temperature threshold) belong to typed domain entities linked to the requirement, because unit semantics differ between domains.

### Entities

Domain objects such as components, nets, or firmware targets.

| Field | Description |
| --- | --- |
| `kind` | A `Kind` registered as an entity kind by its domain |
| `name` | `Text<120>` |
| `status` | `proposed`; `accepted` (`by`); `rejected` (`by`) |
| `data` | Domain payload, validated and normalized by the domain codec |

`status` records the user's decision about the entity, for example accepting a proposed component. It is not a technical validation state. Structural validity is enforced by the domain codec whenever the project is loaded or changed, and the outcomes of checks are recorded as validation results.

### Artifacts

References to files in the project directory. The file content is never stored in the IR. The files are stored in `files/` in the project directory, and `path` is relative to it ([ADR-0017](DECISIONS.md#adr-0017-artifact-file-storage)).

| Field | Description |
| --- | --- |
| `kind` | A registered artifact kind, for example `core.document` |
| `path` | `ProjectPath` |
| `ownership` | `generated` (managed by Text2Create), `user` (never overwritten without approval), `tool_output` |
| `content` | `{ "digest", "size_bytes", "provenance" }` for the last recorded content, or `null` if the file has not been produced yet |

The store writes a file and then records its SHA-256 digest and size with `record_artifact_content`, so the content record exists only for content that was written, and its provenance names the producer. A file is replaced only if it still has the recorded content, and the file of a `user`-owned artifact only by the user. Files are compared with their recorded content when a project is opened; differences (missing, changed, or unrecorded files) are reported and never recorded or repaired automatically.

### Links

Typed links for traceability across records and domains: `kind`, `from`, `to`. Core link kinds:

| Kind | From | To |
| --- | --- | --- |
| `core.satisfies` | Entity, Artifact | Requirement |
| `core.derived_from` | Requirement, Entity, Artifact | Requirement, Entity, Artifact |
| `core.describes` | Artifact | Entity |
| `core.refines` | Requirement | Requirement |

Domains can register additional link kinds with allowed endpoint types. Internal domain structure (for example electrical connections) belongs in domain payloads, not in generic links.

### Plans and Tasks

A plan has `goal`, `status` (`draft`; `approved` (`by`); `archived` (`by`)), and `tasks`.

A task describes planned work only:

| Field | Description |
| --- | --- |
| `title` | `Text<200>` |
| `action` | A registered action kind, for example `embedded.arduino_cli.compile` |
| `depends_on` | Tasks in the same plan; must be acyclic |
| `inputs` | Record references |
| `expected_outputs` | Artifact IDs |
| `addresses` | Confirmed requirement IDs |
| `disposition` | `active`; `skipped` (`by`); `cancelled` (`by`) |

Tasks have no execution status. Whether a task is running, succeeded, or failed is derived from its task runs, which are stored outside the IR. A completed plan is likewise derived, not stored.

### Validation Results

Immutable records of a check that ran against exact artifact contents.

| Field | Description |
| --- | --- |
| `provenance` | A provenance record whose actor is a tool or the system; the tool name and version are recorded there |
| `check` | A registered check kind |
| `subjects` | `{ "artifact", "digest" }`: the exact content that was checked |
| `requirements` | Requirements the check relates to |
| `outcome` | `passed`, `failed`, `error`, `skipped` |
| `summary` | `Text<500>` |
| `diagnostics` | At most 200 entries: `severity` (`error`, `warning`, `info`), `message`, `location` |

A validation result is **stale** when any subject digest differs from the artifact's current content digest. Staleness is computed, never stored.

## Domain Extensions

A domain is a Rust type implementing a domain codec interface, compiled into the application and registered at startup. A domain declares:

- its identifier and schema version;
- the entity, artifact, action, and check kinds it defines;
- link rules (kind name and allowed endpoint record types);
- for each entity kind, a typed Rust payload structure that rejects unknown fields.

The core treats payloads as opaque until the domain codec has processed them. For every entity, the codec decodes the payload into its Rust type, validates it, encodes it again, and reports any record IDs it contains. The core then requires that:

- the re-encoded payload equals the stored payload (stored payloads are always in canonical form);
- every reported record ID exists.

A domain can also check project-wide rules that span records (`check_project`, invariant 13), and it migrates its own payloads between its versions (`migrate_entity`, see [Migrations](#migrations)). Domains evolve independently: each has its own version. A test-only example domain demonstrates the interface in #10, and test-only version 2 domains demonstrate migrations in #11.

## Loading Pipeline

Loading is the only way to obtain a validated project. The loader never writes to the source.

1. **Size limit:** documents larger than 16 MiB are rejected.
2. **Strict JSON parsing:** the input is parsed into a JSON value with a custom parser that rejects:
   - repeated object keys at any depth, including inside domain payloads;
   - non-integer numbers and integers outside the 64-bit range;
   - arrays and objects nested deeper than 127 levels (the recursion limit of the JSON library);
   - trailing data, a byte order mark, and invalid Unicode escapes.
3. **Version checks:** the `format` and `domains` versions are checked before typed parsing, so that a document from a newer application version produces a precise error.
4. **Typed parsing:** the document is parsed into Rust types that reject unknown fields and unknown enum values. Typed parsing reads the original bytes again so that errors report a line and column.
5. **Payload normalization:** each entity payload is processed by its domain codec, as described above.
6. **Invariant checks:** see [Invariants](#invariants).

### Error Locations

Every rejection reports where the invalid data is, as a JSON Pointer (RFC 6901) into the document, for example `/requirements/0/status/by`, together with a message that can be shown to the user. The message contains the pointer. The location is as precise as the stage that rejects the data:

| Stage | Location |
| --- | --- |
| Strict parsing | The value that is invalid; for a repeated key, the object that repeats it |
| Version checks | `/format/version`, or `/domains/<i>/id` or `/domains/<i>/version` |
| Typed parsing | The invalid or unknown field; for a missing field, the object that lacks it; inside a tagged value such as `actor` or `status`, the tagged value |
| Payload normalization | The location inside the payload, for example `/entities/0/data/size` |
| Invariants | The field that violates the rule, for example the reference that does not resolve or the second of two records out of order |
| Domain rules | The location the domain reports |

Change sets report locations in the same way. Write path errors raised by an operation use locations relative to that operation.

Repeated keys must be rejected before conversion to a JSON value because the conversion itself silently keeps only the last value. Data that does not come from Text2Create's own serializer (project files, imports, model responses, tool output) must go through this parser. Frontend command arguments are parsed by Tauri before they reach application code, so this check does not apply to them. JavaScript's `JSON.stringify` cannot produce repeated keys.

## Invariants

A loaded project satisfies all of the following:

1. Record IDs are unique within each collection, and collections are sorted by ID. Task IDs are unique across all plans. `domains` is sorted and unique.
2. Every reference resolves: link endpoints, task dependencies, inputs, outputs, and addressed requirements, validation subjects, superseding requirements, and references inside domain payloads. References from provenance records are historical and are not required to resolve (see [Provenance](#provenance)).
3. Every kind is registered for its use (entity, artifact, action, check, or link) by a domain that is listed in `domains` or is `core`.
4. Link endpoints have record types allowed by the link rule.
5. Task dependencies stay within their plan and are acyclic.
6. Tasks address only confirmed requirements.
7. Every `by` field (confirmed, rejected, superseded, accepted, approved, archived, skipped, cancelled) references a provenance record whose actor is the user.
8. Validation results reference a provenance record whose actor is a tool or the system.
9. References to provenance records (`created`, `modified`, `by`, and the provenance of content records and validation results) resolve. Provenance records that the current state no longer references are allowed and are kept, because provenance is append-only.
10. `rev` is at least 1.
11. Entity payloads are in canonical form.
12. `TaskRunId` references are not resolved in v1, because task runs are stored outside the IR.
13. The project-wide rules of every listed domain hold (`check_project`). They are checked after rules 1–12.

Rules 6–8 are data integrity rules. They prevent, for example, a model-attributed change from confirming a requirement or recording a validation result. They are not an approval workflow, which is defined in #26.

## Write Path

A validated project is obtained only by loading or by creating a new project, and changed only by applying a **change set** ([#43](https://github.com/ensarsnr/Text2Create/issues/43)). The project has no other mutators.

A change set contains the records the change was derived from (`inputs`) and an ordered list of typed operations. It contains no actor, no identifiers of new records, and no timestamps. A change set that comes from outside Text2Create's own code, for example from a model response, is parsed with the strict JSON parser.

**Source.** The source of a change is chosen by the trusted Rust code that knows where the change comes from, through one constructor per source: user edit, user import, model interpretation, model task run, tool task run, or system import. A source cannot be deserialized, so frontend command arguments, change sets, and model output cannot set the actor. The task of a task-run source must exist.

**Applying.** A change set is applied all or nothing:

1. The operations are applied in order to a copy of the project.
2. The core allocates the identifier of every new record and of one provenance record for the change set, and its timestamp, through injected identifier and clock interfaces.
3. The core maintains `rev`, `created`, `modified`, and every `by` field. A record changed by a change set gets one new revision, whatever the number of operations.
4. Entity payloads are normalized by their domain codecs, and the copy must pass the same checks as a loaded project, including every [invariant](#invariants).
5. Only then does the copy replace the project. On any error the project is unchanged.

Provenance records are only appended. An approval check (#26) can inspect the source and the change set before they are applied.

**Operations:**

| Record | Operations |
| --- | --- |
| Project | `set_project_info`, `declare_domain` (at the version of the running build) |
| Requirements | `add_requirement` (`proposed`), `edit_requirement`, `decide_requirement` (confirm or reject), `supersede_requirement` |
| Entities | `add_entity` (`proposed`), `edit_entity`, `decide_entity` (accept or reject) |
| Artifacts | `add_artifact` (without content), `record_artifact_content` |
| Links | `add_link` |
| Plans and tasks | `add_plan` (`draft`, with its tasks), `decide_plan` (approve a draft, or archive), `set_task_disposition` (skip or cancel an active task) |
| Validation results | `record_validation` |

**Rules:**

- Decision operations (`decide_*`, `supersede_requirement`, `set_task_disposition`) are accepted only from the user. `record_validation` is accepted only from a tool or the system.
- Only the user can edit a requirement or entity that is no longer `proposed`; the decision is kept. Superseded requirements cannot be edited or decided again. A plan can be approved only as a draft, archived plans are final, and only active tasks of plans that are not archived can be skipped or cancelled.
- A new record can be given a temporary reference so that later operations of the same change set can refer to it. Temporary references must be declared before they are used, declared only once, used at least once, and refer to the expected record type. References inside domain payloads must use identifiers of existing records.
- Inputs must reference existing records. A change set without operations is rejected.
- v1 has no operations that remove records. Only a rollback, which is not a change set operation, can remove records (see [History and Rollback](#history-and-rollback)).

## History and Rollback

Specified in [ADR-0018](DECISIONS.md#adr-0018-project-history-and-rollback) ([#14](https://github.com/ensarsnr/Text2Create/issues/14)); not implemented yet. The history is stored by `text2create-store`, outside the document; the IR core computes the rolled-back document without I/O.

- **Granularity:** one history entry per saved change set, with its operations in their original order and the previous version of every record it changed.
- **A rollback is a new change.** It is accepted only from the user and is not a change set operation. It keeps every provenance record and adds one with a `restore` activity that names the provenance record of the target state. This activity is added to format version 1 without a version change, because no version has been released.
- **Revisions only increase.** A record whose content differs from the target state gets `rev + 1` and the rollback's provenance record as `modified`; `created` is kept, and unchanged records keep their provenance. A record that returns after an earlier rollback removed it continues from its highest earlier revision. Records created after the target state are removed, except `user`-owned artifacts.
- **The result is validated** with the complete loading checks. A rebuilt state that is incomplete, damaged, or does not match its recorded digest stops the rollback before anything is changed.
- **`user`-owned artifacts are kept**, with their records, recorded content, and files. Generated artifact files are restored from retained content, and only if they still match their recorded content.
- **Limits:** states before the last migration cannot be restored, and effects of irreversible actions outside the project are not undone.

## Unknown Data and Versions

Data is never silently discarded, and data that could not be fully validated is never written.

| Situation | Behavior |
| --- | --- |
| Unknown field, in the core or in a domain payload | Rejected |
| Unknown enum value | Rejected |
| Repeated key, non-integer number, out-of-range integer | Rejected during strict parsing |
| Payload not in canonical form | Rejected (indicates an external edit or a bug) |
| Domain not available in this build | Rejected; the file is not modified |
| Format or domain version newer than supported | Rejected; the file is not modified |
| Format or domain version older than supported | Rejected by loading; opened only after the user approved a [migration](#migrations), which runs after a backup |

There is no mechanism that preserves unknown fields and writes them back, because it would let unvalidated data persist in the IR. Read-only viewing of projects from newer versions is out of scope for v1.

## Migrations

Loading never migrates. A document written with an older format or domain version fails to load with an error that reports that it needs a migration. `Project::migrate` then:

1. parses the document with the strict parser and refuses newer or unknown format and domain versions;
2. runs the format steps from the stored version to the current one, in order, on the JSON value;
3. runs, for each listed domain whose version is older, the domain's payload migration once per version step for every payload of that domain, and normalizes the migrated payloads with the current codec;
4. records each step as a provenance record with the `system` actor and a `migration` activity (`from`, `to`). The component is `project-ir` for format steps and `domain:<id>` for domain steps. The step's record becomes the `modified` provenance, with `rev` increased, of the project record and of every record whose data the step changed: for a format step, every requirement, entity, artifact, link, or plan (including its tasks) that differs before and after the step; for a domain step, every entity whose payload it changed. Validation results keep the provenance of the tool run that produced them. Existing provenance records are kept unchanged;
5. runs the complete loading pipeline on the result.

Migrations are deterministic: the result depends only on the input, the available domains, and the clock and identifier generator of the core. They use no external services. A step that cannot convert data fails with an error that names the step and the location, and never drops or repairs data silently. The input is never modified.

Format version 1 is the first format version, so this build contains no format steps. The framework is tested with synthetic format 0 fixtures (`tests/fixtures/synthetic-format-v0.json`, and `synthetic-format-v0-entities.json` with entities) and test-only version 2 domains.

A migration changes the stored project permanently, so it requires the user's approval (ADR-0011). `Project::pending_migrations` lists the steps without running them, and the store migrates only with a plan the user approved that still matches the project. Until the approval flow ([#26](https://github.com/ensarsnr/Text2Create/issues/26)) exists, the application refuses to open projects that need a migration and never migrates them.

Stored projects are backed up before a migration is saved: the store first migrates in memory, then copies the database into the project's `backups/` directory with SQLite `VACUUM INTO` (which includes committed data that is still in the write-ahead log), verifies the copy, and only then saves the migrated document in one transaction ([ADR-0016](DECISIONS.md#adr-0016-project-ir-error-locations-and-versioned-migrations)). The SQLite schema version of the database (ADR-0015) is separate from the Project IR format version.

## Canonical Serialization

The same project state always serializes to the same bytes:

- struct fields in declaration order; domain payload keys sorted;
- record collections sorted by ID;
- no maps keyed by ID, no hash-ordered collections, no floating-point numbers;
- UTF-8, two-space indentation, LF line endings, one trailing newline.

Fixtures are compared byte for byte. These comparisons also detect dependency changes that would alter JSON output (for example `serde_json` features enabled elsewhere in the dependency tree).

## Security Considerations

- **Untrusted content:** text in the IR is data, never instructions. Model context must show where each record came from and whether it was confirmed.
- **Resource limits:** document size, nesting depth, text lengths, and diagnostics per result are bounded.
- **Paths:** artifact paths are validated lexically in the IR and again when files are accessed. Files are opened relative to directory handles without following symbolic links, so no access leaves the project directory ([ADR-0017](DECISIONS.md#adr-0017-artifact-file-storage)).
- **Identifiers:** allocated by the core, so input cannot create collisions or overwrite existing records.
- **Secrets:** API keys and credentials are never stored in the IR. Prompts and model responses are referenced by digest.
- **Irreversible actions:** a rollback does not undo effects outside the project, such as flashing firmware to a device. The history marks them, and a rollback plan states that they are not undone (ADR-0018, #39).

## Example

An excerpt of an illustrative project for the first workflow (ESP32 climate fan controller), in canonical form. Only the target board, ESP32 DevKit V1, is a confirmed choice. The sensor, its wiring, and the firmware target identifier are illustrative and remain `proposed`; the reference specification is defined in [#31](https://github.com/ensarsnr/Text2Create/issues/31). The `electronics` and `embedded` payloads only illustrate the shape; their schemas are not designed yet.

```json
{
  "format": {
    "schema": "text2create.project-ir",
    "version": 1
  },
  "domains": [
    {
      "id": "electronics",
      "version": 1
    },
    {
      "id": "embedded",
      "version": 1
    }
  ],
  "provenance": [
    {
      "id": "prv_01a21c90-0000-7000-8000-000000000003",
      "at": "2026-11-27T10:05:00.000Z",
      "actor": {
        "type": "user"
      },
      "activity": {
        "type": "user_edit"
      },
      "inputs": [],
      "model": null
    }
  ],
  "requirements": [
    {
      "id": "req_01a21c90-0000-7000-8000-000000000014",
      "provenance": {
        "rev": 2,
        "created": "prv_01a21c90-0000-7000-8000-000000000002",
        "modified": "prv_01a21c90-0000-7000-8000-000000000003"
      },
      "kind": "constraint",
      "priority": "must",
      "status": {
        "state": "confirmed",
        "by": "prv_01a21c90-0000-7000-8000-000000000003"
      },
      "title": "Use an ESP32 DevKit V1 board",
      "statement": "The controller is an ESP32 DevKit V1 development board.",
      "origin": {
        "type": "user",
        "quote": "My board is an ESP32 DevKit V1."
      },
      "acceptance": []
    }
  ],
  "entities": [
    {
      "id": "ent_01a21c90-0000-7000-8000-000000000022",
      "provenance": {
        "rev": 1,
        "created": "prv_01a21c90-0000-7000-8000-000000000002",
        "modified": "prv_01a21c90-0000-7000-8000-000000000002"
      },
      "kind": "electronics.component",
      "name": "U2 temperature and humidity sensor",
      "status": {
        "state": "proposed"
      },
      "data": {
        "designator": "U2",
        "interface": "i2c",
        "part": "SHT31 breakout (illustrative)",
        "role": "sensor"
      }
    }
  ]
}
```

The excerpt omits the remaining fields and records. A complete document must contain every top-level field and pass every invariant.

## Specification Decisions

Decisions taken while specifying v1, with the alternatives that were rejected. Changing one of them requires updating this document and, where affected, the ADRs.

| # | Topic | Decision | Rejected alternative |
| --- | --- | --- | --- |
| 1 | Provenance retention | Append-only and kept indefinitely, also after a rollback (ADR-0018) | Keeping only provenance referenced by the current state: loses history, and restored records could no longer reference their provenance |
| 2 | Decision status on entities | Entities have `status` (`proposed`, `accepted`, `rejected`), recording the user's decision. It is separate from technical validation (see [Entities](#entities)). | No status; rely on provenance only |
| 3 | Write path (change sets, operations, provenance creation) | A separate issue, [#43](https://github.com/ensarsnr/Text2Create/issues/43), completed before #12 | Part of #10 |
| 4 | Code location | A separate Rust crate without a Tauri dependency, as a Cargo workspace member under `src-tauri/` | A module inside the existing application crate |
| 5 | Projects from newer IR versions | Rejected without modifying the file. Read-only viewing is out of scope for v1. | Read-only viewing in v1 |
| 6 | Unicode normalization of text and paths | Not required and not applied in v1 (see [Scalar Types](#scalar-types)) | Requiring NFC or normalizing automatically |
| 7 | Embedded (firmware) domain schema | Its own issue, [#44](https://github.com/ensarsnr/Text2Create/issues/44). Electronics (#30) owns boards, components, pins, nets, and power; embedded owns firmware targets, toolchains, build configuration, and the mapping of firmware signals to pins. | Including it in #30 |
| 8 | Error locations | JSON Pointers, recorded only after a parse has failed ([ADR-0016](DECISIONS.md#adr-0016-project-ir-error-locations-and-versioned-migrations)) | Line and column only; a JSON Schema validator |
| 9 | Migration of older documents | On the JSON value, recorded as `system` provenance per step, then fully revalidated | Keeping Rust types for every old version; migrating without provenance |

## Open Questions

These remain open and belong to later issues:

- How should the user reconcile changes made to artifacts outside Text2Create? They are detected and reported (#13); recording or reverting them needs a user decision in the interface.
- Where should cross-domain numeric constraints (for example budget or timing) live, given that the core has no unit type?
