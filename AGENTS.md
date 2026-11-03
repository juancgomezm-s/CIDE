# Working on JC-IDE

Read `README.md`, `docs/DECISIONES.md`, and the relevant milestone in `docs/PLAN.md`.

F1 is implemented; read `docs/F1_DOCUMENT_VIEWS_TRANSACTIONS.md` for actual ownership, transaction APIs, selection mapping and validation. F2 is also implemented: read `docs/F2_RANGES_INDEX_MEMORY.md` and its raw measurements. F3 is implemented with local evidence: read `docs/F3_PRESENTATION_TASKS.md`. Preserve read-only composition, incremental terminal output, bounded owned snapshots, cooperative cancellation and versioned delivery. Project Search Plus literal scanning and versioned replacement previews are implemented: read `docs/S1_PROJECT_SEARCH.md`. S0 literal is implemented with cooperative range scanning and default full Unicode 17 case folding: read `docs/S0_DOCUMENT_SEARCH.md`. The shared decoder is optimized with a borrowed scalar iterator and reusable scan buffers: read `docs/S0_DECODER_OPTIMIZATION.md`. Expanded corpus/process evidence and the bounded cold-line layout correction are recorded in `docs/M2_ACCEPTANCE_EVIDENCE.md`. Read `docs/DIFF_MODEL_READ_ONLY.md` and `docs/GIT_READ_ONLY_PROVIDER.md` before connecting diff: the pure bounded model and Windows HEAD/index blob reader are implemented/tested. Versioned Git delivery and the read-only active-file TUI are now connected: read `docs/GIT_DIFF_DELIVERY.md` and `docs/GIT_DIFF_PRESENTATION.md`. Preserve one in-flight comparison, pending metadata only, artifact reservation through poll, explicit executable configuration and stale rejection before presentation/navigation. Keep Git optional, bounded, read-only and off the input/render path; do not weaken its process cleanup. `docs/M2B_SCENE_STUDY.md` and `prototypes/m2b/` are a static study, not a GUI backend. Dense line metadata now uses one blocked index with native bases and packed relative offsets/EOL, replacing the flat width-promoting route. Read `docs/M2_BLOCK_LINE_INDEX.md` and `docs/M2_UNDO_PIPELINE_PROBE.md`; the experimental comparator is historical at `191bd82`. Full reachable source arenas are reused during collection. Dense edited native peak is 43.73/43.82 MiB in two local captures; global RAM/physical-display gates stay open. Keyboard/menu dispatch availability is centrally checked in TUI; read `docs/M2_KEYBOARD_MATRIX_AUDIT.md` before changing routing. Suffix/cold/native/Git auxiliary comparisons are recorded; full M2A and global physical-display latency/RAM acceptance remain open. Read `docs/SAVE_WINDOWS_CONTENTION.md` before changing save behavior; preserve bounded retry/conflict checks and recovery temporaries. Preserve range-based reads, revision/encoding-aware projection, sparse checkpoints and configured memory budgets. Current priority is M2-F, defined by `docs/AUDITORIA_ECODE_ARQUITECTURA.md` and `docs/ARQUITECTURA.md`. Read both before extending editor behavior. ecode/eepp is an implementation reference; Sublime Text remains the visual reference. The existing split checkpoint is functional evidence, not architectural/performance acceptance.

## Invariants

- The editor core stores arbitrary bytes. Never decode a source file with a lossy conversion.
- Source encoding is explicit per document/project. The user's actual legacy page is still unconfirmed.
- Byte offsets, Unicode scalar offsets, grapheme positions, LSP positions, and visual columns are different types of coordinates. Convert explicitly at boundaries.
- The core is language independent. Progress belongs in `jc-progress` / `progress/`, and other language services must remain optional.
- Preserve original bytes, line endings, and terminal newline unless an edit explicitly changes them.
- Retain upstream copyright/license notices and identify actual ports separately from new code.
- Keep the first milestone small: no UI dependency or language-server process in the headless core.

## Development

Juan explicitly authorized continuous in-plan development: do not wait for another
"continue" after an intermediate commit. Close each coherent block with integration
and relevant local validation, then activate the next independent planned block.
Before ending a turn, leave the next block genuinely active or identify a concrete
block requiring Juan's decision; a completed intermediate commit alone is not a
reason to stop. Reuse up to three native agents total when contracts permit useful
independent construction. Keep one integrator for shared APIs/exports/manifests,
commands, plan/validation and commits; serialize heavy checks and benchmark windows.
This authorizes continuity, not expansion beyond the product plan or its gates.

Build broader coherent implementation blocks before validation. Run the relevant
local checks when closing each block; use targeted checks during implementation
only to resolve a concrete blocker. Do not repeat passing suites after minor
adjustments unless a new failure or material concern justifies it.

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

- Read `docs/M2_BLOCK_LINE_INDEX.md`: LineIndex is one blocked catalog, at most 256 u16 relative/EOL records per block, with lossless usize bases. Catalog slack is capped at 32 KiB logical header charge; large deletions release excessive idle headers. Preserve heap-only accounting and native-offset geometry. Sparse long-line metadata and warm ordinal lookup can cost more; no universal speed/RAM claim. Read `docs/GIT_AUXILIARY_MEASUREMENT.md` for process sampling limits. Build complete blocks before their serial validation window.

- Range reads use the block prefix piece index. Do not restore whole-document decode/index builds in movement, viewport rendering or range edit commands.
- TextProjection is shared per document; direct/inverse changes update EOL events and sparse scalar checkpoints. Preserve invalid UTF-8 grouping, literal mid-file BOM and legacy offsets.
- Layout cache quota is 1 MiB/document; line metadata is accounted separately. History defaults to 8 MiB logical charge and 1024 steps. These are not private-process RAM guarantees.
- Source collection relocates reachable intervals without clearing retained undo/redo. Explicit compact_storage still clears history.
- DocumentRead is a borrowed revision-consistent read; use the bounded owned snapshots/queues of F3 and S1 for workers, never a cloned editor buffer.
- Initial cold line layout and line splitting may allocate/decode the affected line; profile that peak and metadata suffix shifts. Do not claim <50 MiB or full UI latency from model-only benchmarks.

## M2-F architecture gates

- The blocked index is integrated after measured selection; no flat fallback or second editing model remains. See `docs/M2_BLOCK_LINE_INDEX.md` for the complete migration and `docs/M2_UNDO_PIPELINE_PROBE.md` for paired stage timings and VT repeats. Historical flat/candidate data remain reproducible at their commits. Local dense RAM below 50 MiB does not accept global services/machines or physical display latency. Do not repeat completed measurements without a material question.

- Build transactional edits and document/view ownership before more navigation/providers. Multi-edit commands validate against an expected revision and publish one undoable change with a complete position map; workers and frontends must not mutate live buffers directly.
- Text navigation must reject the interior of CRLF. Byte, scalar, grapheme and display coordinates are not interchangeable.
- Remove whole-document copying/decoding/index rebuilding from steady-state movement and rendering. Use range access, revision-aware indices, bounded caches and visible rows. Measure cold operations separately.
- Rendering consumes read-only presentation; update viewport before rendering. Background results carry revisions/generations and use bounded, cancelable queues.
- Measure local performance now; CI remains deferred to M5. Compare memory and latency before selecting a storage structure, release optimization level or GUI backend. Do not claim that this target architecture is already implemented.
- Preserve byte/encoding/save guarantees while replacing flawed ownership and notification paths. Each migration removes its replaced route; do not retain two production editing models.


## Current CPU priority

Repository status provider/controller are implemented and locally validated; read
docs/GIT_STATUS_READ_ONLY.md, docs/GIT_STATUS_DELIVERY.md and
docs/GIT_STATUS_PRESENTATION.md. Status TUI is integrated; CPU profiling is closed.
The following CPU requirements remain constraints, not a request to repeat it:
follow the latest CPU block
in PLAN/CONTINUAR, profile before selecting parallelism, bounded configurable
concurrency/cancellation/backpressure, deterministic versioned results, no nested
pools or large extra copies. Preserve CPU/RAM headroom for Qwen; do not manipulate
other processes/system settings. Mark concurrent load unknown if not safely verified.


## CPU closure and next authorized work

CPU file execution and TUI --search-workers are implemented and locally validated;
read docs/CPU_PERFORMANCE.md, docs/CPU_PROJECT_EXECUTOR.md and
docs/CPU_RUNTIME_OPTIONS.md. Default2 total workers includes coordinator; requested
1..4 capped by available-minus4 (minimum1), no OS affinity/priority changes.
One engine/window1 and helpers, global quotas/order, bounded staging, Request Arc
released before idle, no extra open-source copies or nested F3 pools. Preserve
source-byte quota semantics and separate EOF sentinel, snapshot revisions and undo.
274 distinct tests passed in selective closure; do not claim one full274 run.
CPU total increased in the measured tradeoff; Qwen load unknown and physical UI
latency still open. Codegen s/2/3 already compared: keep s, do not repeat that
profile or prolong CPU optimization without a concrete new question. The next
authorized increment, repository-status TUI presentation, is now integrated:
read docs/GIT_STATUS_PRESENTATION.md. Latest closure: one full workspace run of
290 passed, fmt/Clippy all-targets/release. Preserve poll_tasks as sole completion
acceptance route, context invalidation before prepare, bounded visible rows,
escaped opaque paths, explicit refresh/cancel, read-only rendering and terminal
recovery even for stale worker panics. No file navigation from status yet.
Build complete functional blocks before their validation window; do not repeat
passing suites for documentation or minor changes without a concrete concern.

## Rectangular selection checkpoint

Read docs/M2_RECTANGULAR_SELECTION.md. A real one-action rectangle is integrated
for an explicit one-cell repertoire plus TAB4 and virtual cells; this does NOT
close the general Unicode column-mode gate. Reject unsupported scalars, invalid
decoding, line/row/column/source/staging limits before mutation. Source byte caret/
anchor remain unique; visual columns/scroll are separate metadata per view.
selection_for_pane delegates to ViewState, with no linear range for a rectangle.
Content or encoding changes clear rectangular geometry/anchor in affected views.
One apply_text_edits batch, one undo, no disk write; internal rectangular clipboard
does not imply OS clipboard. Visible preparation expands tabs/padding; renderer
is read-only and never treats VIRTUAL_BYTE as an edit/source-match position.
310 distinct local tests passed in selective closure, not a single310 run;
fmt/Clippy/release passed. Next active M2A block: generic join/split lines, using
bounded range proposals and existing newline insertion, no second edit engine.
