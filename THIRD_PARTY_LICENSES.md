# Third-Party Licenses

Text2Create is licensed under the [Apache License 2.0](LICENSE). This document explains how third-party dependencies and their licenses are tracked.

## Current State

**No third-party runtime dependencies have been adopted yet**, and no dependency license audit has been performed, because there is no application code.

Repository tooling used only in CI is listed below for transparency. These tools are not distributed with Text2Create.

| Tool | Used for | License |
| --- | --- | --- |
| [actions/checkout](https://github.com/actions/checkout) | Checking out the repository in CI | MIT |
| [markdownlint-cli2-action](https://github.com/DavidAnson/markdownlint-cli2-action) | Markdown linting in CI | MIT |
| [yamllint](https://github.com/adrienverge/yamllint) | YAML linting in CI | GPL-3.0 |

## Policy for Adding Dependencies

Every new dependency must be justified in its pull request, including:

1. **Purpose:** why it is needed and why existing code or dependencies are not sufficient.
2. **License:** the SPDX license identifier, verified from the dependency's source.
3. **Compatibility:** confirmation that the license is compatible with Apache 2.0 for the way the dependency is used.
4. **Maintenance:** evidence that the dependency is maintained, and its security history.
5. **Alternatives:** other options considered.

### License Compatibility Guidelines

| Category | Examples | Guidance |
| --- | --- | --- |
| Permissive | MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, ISC, Zlib | Generally acceptable |
| Weak copyleft | MPL-2.0, LGPL | Requires review of how the dependency is used and distributed |
| Strong copyleft | GPL, AGPL | Must not be linked into or bundled with Text2Create without explicit review |
| Unknown or no license | — | Not acceptable |

Some candidate engineering tools use copyleft licenses (for example KiCad and ngspice components). The intended approach is to invoke such tools as separate, user-installed programs rather than linking or bundling them. Each integration will be reviewed before it is adopted.

This document is not legal advice. When in doubt, raise the question in the pull request.

## Planned Tracking Process

When application dependencies are introduced (Milestone 1 and later):

- Rust dependency licenses will be checked automatically in CI (a tool such as `cargo-deny` is being considered).
- JavaScript dependency licenses will be checked with an equivalent tool.
- The table below will be maintained, and generated where practical, listing each direct dependency, its version, and its license.
- Required attribution notices will be included in distributed builds.

## Dependency Inventory

| Dependency | Version | License | Purpose |
| --- | --- | --- | --- |
| None yet | — | — | — |

## Integrated External Tools

External programs invoked by Text2Create, rather than linked into it, will be listed here with their licenses once an integration is implemented.

| Tool | License | Integration status |
| --- | --- | --- |
| None yet | — | — |
