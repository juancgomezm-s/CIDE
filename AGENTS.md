# Working on JC-IDE

Read `README.md`, `docs/DECISIONES.md`, and the relevant milestone in `docs/PLAN.md`.

F1 is implemented; read `docs/F1_DOCUMENT_VIEWS_TRANSACTIONS.md` for actual ownership, transaction APIs, selection mapping and validation. F2 is also implemented: read `docs/F2_RANGES_INDEX_MEMORY.md` and its raw measurements. F3 is implemented with local evidence: read `docs/F3_PRESENTATION_TASKS.md`. Preserve read-only composition, incremental terminal output, bounded owned snapshots, cooperative cancellation and versioned delivery. Project Search Plus literal scanning and versioned replacement previews are implemented: read `docs/S1_PROJECT_SEARCH.md`. Next: close bounded document search/S0 and Unicode case folding; full M2A and global physical-display latency/RAM acceptance remain open. Read `docs/SAVE_WINDOWS_CONTENTION.md` before changing save behavior; preserve bounded retry/conflict checks and recovery temporaries. Preserve range-based reads, revision/encoding-aware projection, sparse checkpoints and configured memory budgets. Current priority is M2-F, defined by `docs/AUDITORIA_ECODE_ARQUITECTURA.md` and `docs/ARQUITECTURA.md`. Read both before extending editor behavior. ecode/eepp is an implementation reference; Sublime Text remains the visual reference. The existing split checkpoint is functional evidence, not architectural/performance acceptance.

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

## M2A dual-pane checkpoint and local continuation

- Start by reading `docs/M2_NAVIGATION_PANES_GIT.md` and `docs/CONTINUAR_EN_EQUIPO.md`.
- `PaneManager` in `jc-app` now drives two TUI groups, pin, navigation history, independent caret/selection/viewport and linked vertical scrolling. Byte-edit geometry also rebases inactive carets and history through undo/redo. See the current evidence in `docs/VALIDACION.md`; ABL include/RUN navigation, completion, diff and Git operations remain pending.
- Never clone `Document`, `FileSession` or undo history to implement split views. F1 separates DocumentId from ViewId; TabId resolves the catalog entry to its document. F1 removed the `EditorDocument`/`PaneView` mirror: each persistent view now owns its caret/selection/viewport exactly once. Keep that ownership and use `apply_text_edits` for external proposals; mutable editor projections are internal only. Committed Git snapshots are immutable separate read models.
- Pin and scroll linkage are independent. Navigation history is per group; keyboard commands are registered centrally only once a working handler exists.
- Git status/diff can start read-only during M2; stage/unstage/commit/push/pull/merge and three-way conflict resolution belong to M5 and must require explicit user action.
- Use local Rust formatting/tests for validation when available; do not activate or run GitHub Actions during M2–M4.

## F2 contracts

- Range reads use the block prefix piece index. Do not restore whole-document decode/index builds in movement, viewport rendering or range edit commands.
- TextProjection is shared per document; direct/inverse changes update EOL events and sparse scalar checkpoints. Preserve invalid UTF-8 grouping, literal mid-file BOM and legacy offsets.
- Layout cache quota is 1 MiB/document; line metadata is accounted separately. History defaults to 8 MiB logical charge and 1024 steps. These are not private-process RAM guarantees.
- Source collection relocates reachable intervals without clearing retained undo/redo. Explicit compact_storage still clears history.
- DocumentRead is a borrowed revision-consistent read; use the bounded owned snapshots/queues of F3 and S1 for workers, never a cloned editor buffer.
- Initial cold line layout and line splitting may allocate/decode the affected line; profile that peak and metadata suffix shifts. Do not claim <50 MiB or full UI latency from model-only benchmarks.

## M2-F architecture gates

- Build transactional edits and document/view ownership before more navigation/providers. Multi-edit commands validate against an expected revision and publish one undoable change with a complete position map; workers and frontends must not mutate live buffers directly.
- Text navigation must reject the interior of CRLF. Byte, scalar, grapheme and display coordinates are not interchangeable.
- Remove whole-document copying/decoding/index rebuilding from steady-state movement and rendering. Use range access, revision-aware indices, bounded caches and visible rows. Measure cold operations separately.
- Rendering consumes read-only presentation; update viewport before rendering. Background results carry revisions/generations and use bounded, cancelable queues.
- Measure local performance now; CI remains deferred to M5. Compare memory and latency before selecting a storage structure, release optimization level or GUI backend. Do not claim that this target architecture is already implemented.
- Preserve byte/encoding/save guarantees while replacing flawed ownership and notification paths. Each migration removes its replaced route; do not retain two production editing models.
