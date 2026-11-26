# Contributing to Text2Create

Thank you for your interest in Text2Create. The project is at its foundation stage, so feedback on the architecture, the Project IR concept, and the MVP scope is especially valuable right now.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Language

All public project content must be written in **English**: code, comments, documentation, commit messages, branch names, issues, pull requests, and reviews.

## Ways to Contribute

- Report bugs or documentation problems
- Propose features or design changes
- Discuss open questions in the [Decision Log](docs/DECISIONS.md)
- Improve documentation
- Contribute code once application development begins (see the [Roadmap](docs/ROADMAP.md))

## Reporting Bugs

1. Search [existing issues](https://github.com/ensarsnr/Text2Create/issues) to avoid duplicates.
2. Open a new issue using the **Bug report** template.
3. Include clear reproduction steps, expected and actual behavior, and your environment.

**Do not report security vulnerabilities in public issues.** Follow [SECURITY.md](SECURITY.md) instead.

## Proposing Features

1. Search existing issues first.
2. Open an issue using the **Feature request** template. Describe the problem before the solution.
3. For architectural changes, use the `architecture` label and consider proposing an ADR in [docs/DECISIONS.md](docs/DECISIONS.md).
4. Wait for discussion before starting significant work, so that effort is not wasted on changes that do not fit the project direction.

## Branches

`main` is protected and must always be stable. All changes go through pull requests from a branch.

Branch names use the format `<type>/<short-description>` in lowercase kebab-case, where `<type>` matches the Conventional Commit types below:

```text
feat/project-manager
fix/project-loading
docs/architecture
test/project-ir
refactor/provider-interfaces
chore/github-workflows
ci/markdown-lint
security/permission-validation
```

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>(<optional scope>): <description>
```

| Type | Use for |
| --- | --- |
| `feat` | A new feature |
| `fix` | A bug fix |
| `docs` | Documentation only |
| `test` | Adding or updating tests |
| `refactor` | Code changes that neither fix a bug nor add a feature |
| `perf` | Performance improvements |
| `build` | Build system or dependency changes |
| `ci` | CI configuration |
| `chore` | Maintenance tasks |
| `security` | Security hardening or vulnerability fixes |

Examples:

```text
feat: add project manager
fix: resolve project loading issue
docs: update architecture documentation
test: add project validation tests
refactor: simplify provider interfaces
chore: configure GitHub Actions
security: restrict tool execution permissions
```

Guidelines:

- Use the imperative mood ("add", not "added").
- Keep the subject line under about 72 characters, without a trailing period.
- Use the body to explain *why* the change is needed when it is not obvious.
- Mark breaking changes with `!` after the type (for example `feat!: change IR schema`) and describe them in the body.

## Pull Requests

1. Keep pull requests focused on one change. Do not mix unrelated changes.
2. Fill in the pull request template:
   - Purpose
   - Summary of changes
   - Related issue, if applicable
   - Testing performed
   - Screenshots, when relevant
   - Security or compatibility considerations
3. Use a Conventional Commit style title. With squash merge, the pull request title becomes the commit message on `main`.
4. Make sure all CI checks pass.
5. Address review feedback with additional commits. They will be squashed on merge.

Pull requests are merged with **squash merge** only. Head branches are deleted automatically after merging.

## Running Checks

Run the repository checks locally before opening a pull request:

```sh
python3 scripts/check_repository.py
npx markdownlint-cli2 "**/*.md"
yamllint .
```

See the [Development Guide](docs/DEVELOPMENT.md) for prerequisites. Application build and test commands will be added when application code exists.

## Updating Documentation

Documentation is part of the change, not an afterthought:

- Update relevant documentation in the same pull request as the change it describes.
- Update [docs/PROJECT_STATUS.md](docs/PROJECT_STATUS.md) when something moves from planned to implemented.
- Record significant technical decisions as ADRs in [docs/DECISIONS.md](docs/DECISIONS.md).
- Never describe planned features as implemented.
- Update [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) when adding a dependency.

## Dependencies

New dependencies require justification in the pull request: why it is needed, its license, its maintenance status, and alternatives considered. Prefer fewer, well-maintained dependencies.

## Security and Secrets

- Never commit API keys, tokens, passwords, private keys, or personal information.
- Do not include real credentials in tests, examples, or screenshots.
- Changes to tool execution, approvals, file access, or secret handling should be labeled `security` and explain their impact.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE), as described in section 5 of the license.
