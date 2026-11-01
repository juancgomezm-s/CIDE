# Working on JC-IDE

Read `README.md`, `docs/DECISIONES.md`, and the relevant milestone in `docs/PLAN.md`.

## Invariants

- The editor core stores arbitrary bytes. Never decode a source file with a lossy conversion.
- Source encoding is explicit per document/project. The user's actual legacy page is still unconfirmed.
- Byte offsets, Unicode scalar offsets, grapheme positions, LSP positions, and visual columns are different types of coordinates. Convert explicitly at boundaries.
- The core is language independent. Progress belongs in `jc-progress` / `progress/`, and other language services must remain optional.
- Preserve original bytes, line endings, and terminal newline unless an edit explicitly changes them.
- Retain upstream copyright/license notices and identify actual ports separately from new code.
- Keep the first milestone small: no UI dependency or language-server process in the headless core.

## Development

Local validation commands, used selectively when the local toolchain is available:

- `cargo fmt --all -- --check`
- `cargo test --workspace --locked --offline`
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
- `cargo build --workspace --release --locked --offline`

**CI policy: GitHub Actions is deferred to M5.** Do not enable `push` or `pull_request` triggers, do not invoke the manual workflow, and do not spend remote CI runs chasing intermediate M2–M4 failures unless the repository owner explicitly asks for one. If this environment lacks Rust or another required dependency, record the validation gap and continue with best-effort development.

Introduce dependencies only when they solve a concrete requirement and record the cost/benefit.
Use tests that check observable behavior and regressions, not tests that duplicate implementation details.

Do not describe ABL scripts as verified until they have actually compiled and run on the stated OpenEdge version.
Do not claim a GUI, parser, LSP, debugger, RAM target, or Windows validation merely because a profile or plan exists.
Keep `docs/VALIDACION.md` consistent with observed evidence and material limitations.

## M2A — Lightweight ABL commands

- ABL Toggle Comment defaults to /* ... */ on lines and selections. Keep source encoding, BOM, EOL, and one logical undo step.
- Ctrl+D duplicates, Ctrl+Shift+D deletes the line. Tab/Shift+Tab indent/outdent only in Editor; other focused controls keep their navigation semantics.
- Database/Schema shortcut is Ctrl+Alt+D to avoid conflict with Delete Line.
- Column mode must represent rectangular visual positions and support correct clipboard/undo before being enabled.
- Search Plus should use bounded, cancelable scans and typed results, with no permanent indexer by default.
- Treat M2 edits as unvalidated unless compiled/tested locally; GitHub Actions remains dormant until M5.
- Read docs/ABL_EDITOR_COMMAND_SCOPE.md and docs/M2_SEARCH_PLUS.md.

## Multi-language architecture boundary

- Read `docs/EXTENSION_ARCHITECTURE.md` before adding language-specific features.
- Universal operations (byte editing, selection, undo/redo, search, indentation mechanics, column mode, menus and keymaps) belong to generic core/application code.
- Language-specific delimiters are profile metadata in `jc-languages`. Do not add `if language == "abl"` cases to `jc-app` for syntax rules.
- ABL tokens, templates, USING, OO code actions, XREF, schema and AVM integrations must be moved to/provided by the optional internal `progress.abl` extension when that provider API is ready. No Python or external plugin host before post-M5.
- The current `Progress*` commands in the fixed enum are migration debt, not evidence of an extension loader. Keep them functional/disabled according to context until namespaced registration is implemented.
- Extension-proposed edits must eventually validate the expected document revision and apply as a single undo transaction; no provider writes directly into buffers or disk.
- Do not run GitHub Actions while developing M2–M4; M5 owns remote CI/hardening.
