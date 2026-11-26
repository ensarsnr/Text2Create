# Security Policy

## Project Status

Text2Create is at its foundation stage. There is no released software yet, so there are no supported versions. This policy describes how to report issues today and the security principles the project is designed around.

| Version | Supported |
| --- | --- |
| No releases yet | Not applicable |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public issues, pull requests, or discussions.**

Report vulnerabilities privately through GitHub's private vulnerability reporting:

1. Go to the repository's [Security tab](https://github.com/ensarsnr/Text2Create/security).
2. Select **Report a vulnerability** ([direct link](https://github.com/ensarsnr/Text2Create/security/advisories/new)).
3. Describe the issue, its impact, and steps to reproduce it.

Helpful information includes:

- The affected file, component, or workflow
- Steps to reproduce, or a proof of concept
- The potential impact
- Any suggested mitigation

Maintainers will acknowledge reports as soon as reasonably possible, keep reporters informed of progress, and credit reporters in the advisory unless they prefer otherwise. As a volunteer-run project at an early stage, we cannot guarantee specific response times.

Please give maintainers reasonable time to address an issue before disclosing it publicly.

## Security Expectations

Text2Create is intended to execute AI-planned operations on a user's machine. The following principles are design requirements for the application.

### Secret Management

- Secrets (API keys, tokens, passwords, private keys) must never be committed to the repository, including in tests, examples, fixtures, screenshots, or logs.
- At runtime, secrets are intended to be stored in the operating system's secure storage (planned: macOS Keychain), not in plain-text configuration files or the project database.
- Secrets must never be sent to the frontend, written to logs, included in AI prompts, or stored in the Project IR.
- If a secret is committed accidentally, treat it as compromised: revoke and rotate it immediately. Removing it from Git history alone is not sufficient.

### Least Privilege

- Each component, tool adapter, and AI provider receives only the access it needs.
- File access is limited to the project directory unless the user explicitly grants more.
- The frontend can only invoke explicitly permitted core commands.
- CI workflows run with minimal token permissions.

### Approval for Consequential Operations

Operations with significant or irreversible effects require explicit user approval before they run. These include:

- Writing, modifying, or deleting files outside the project directory
- Running shell commands or external programs not covered by an approved tool definition
- Network requests to services other than the configured AI providers
- Flashing firmware, or controlling physical hardware such as motors and actuators
- Installing software or changing system configuration
- Any operation that may incur costs

The user should be shown what will happen, with enough detail to make an informed decision. Approvals and executed actions should be recorded.

### External Input and Prompt Injection

AI models can be manipulated by instructions embedded in the content they process. Text2Create treats the following as **untrusted data**, never as instructions:

- File contents, including imported projects and datasheets
- Web pages and downloaded content
- Output from external tools
- Responses from AI models

Mitigations the design must include:

- Model output cannot grant itself permissions or bypass approvals.
- Tool calls proposed by a model are validated against the tool's declared parameters and permissions before execution.
- Commands are executed with structured arguments, not by building shell strings from model output.
- Data written into the Project IR from models or external sources is validated.
- Users can see which content influenced a plan or action.

### Dependencies and Supply Chain

- New dependencies require justification and license review (see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)).
- GitHub Actions are pinned to full commit SHAs and updated through Dependabot.
- Dependabot alerts and security updates are enabled for the repository.

## Scope

This policy covers the contents of this repository. Vulnerabilities in third-party tools (for example AI providers, KiCad, or ESP-IDF) should be reported to those projects directly.
