# Documentation index

[日本語](README.md) | **English**

| Document | English | Japanese | Content |
|---|---|---|---|
| Project overview | [README.en.md](../README.en.md) | [README.md](../README.md) | Install, usage, viewer CLI, development layout |
| LLM onboarding | [ONBOARDING.en.md](ONBOARDING.en.md) | [ONBOARDING.md](ONBOARDING.md) | Prerequisites, constraints, references, trial tasks, operations |
| Release procedure | [RELEASING.en.md](RELEASING.en.md) | [RELEASING.md](RELEASING.md) | Weekly check, version decision, tag / Release, asset verification |
| Changelog | [CHANGELOG.md](../CHANGELOG.md) | [CHANGELOG.ja.md](../CHANGELOG.ja.md) | Per-version changes (English canonical) |
| ADR index | [adr/README.en.md](adr/README.en.md) | [adr/README.md](adr/README.md) | Design decisions with English summaries |
| Versioning policy | [adr/0004-versioning-and-release-policy.en.md](adr/0004-versioning-and-release-policy.en.md) | [adr/0004-versioning-and-release-policy.md](adr/0004-versioning-and-release-policy.md) | SemVer, 0.x, patch/minor/major classification |
| Bilingual docs policy | [adr/0005-bilingual-documentation.en.md](adr/0005-bilingual-documentation.en.md) | [adr/0005-bilingual-documentation.md](adr/0005-bilingual-documentation.md) | Japanese default, `*.en.md`, synchronization and checks |
| Viewer CLI requirements | — | [requirements/session_viewer_cli.md](requirements/session_viewer_cli.md) | F01–F10 / N01–N05 and acceptance cases (Japanese only) |
| Porting work record | — | [workdoc_session_jsonl_compactor_rust_port.md](workdoc_session_jsonl_compactor_rust_port.md) | Porting rationale, module mapping, DOD (Japanese only) |

## Language rules

- Documents read continuously by users and contributors are maintained as Japanese/English pairs.
- Japanese is the default; English versions live in `*.en.md`.
- Existing ADRs 0001–0003, requirements, and the work record keep Japanese as canonical;
  the ADR index provides English summaries.
- See [ADR 0005](adr/0005-bilingual-documentation.en.md) for details.
