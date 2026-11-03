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

## Join/Split checkpoint and active admission

Read docs/M2_LINE_COMMANDS.md. Join proposes only separator/ASCII-indent cuts,
bounded to1024 boundaries/2MiB affected source before plan allocation, then one
apply_text_edits batch. Do not claim that preflight as total F2 work/RAM bound.
Raw byte-to-line ordinal metadata does not validate scalar/CRLF caret positions.
Split delegates to insert_newline; Editor Enter is centrally registered and
Ctrl+Shift+J joins while Ctrl+J retains bottom-panel semantics. Preserve final
newline/BOM, explicit encoding, shared views/one undo and rectangle rejection.
319 distinct tests in selective/targeted closure; fmt/Clippy/release approved.
Next active block is read-only regex admission for shared Search Plus, not an
enabled regex or an extension/AVM implementation. Do not install dependencies,
enable a decorative switch or repeat completed profiles. Design captures and
replacement semantics before enabling a regex editing path.

Regex admission is complete; read docs/M2_REGEX_ADMISSION.md. The next concrete
decision is whether Juan authorizes adding/pinning a maintained Rust regex engine
(candidate regex-automata0.4.18) despite the current no-install constraint, or
prefers another M2A task with regex disabled. Do not fetch crates or invent a new
regex parser to bypass that constraint. No implementation/runtime validation is
implied by the admission. Preserve literal full folding, typed source boundaries,
real cooperative work limits and revision-safe replacement; project preview must
show actual match content rather than the regex query. No new suites/profiles for
this documentation-only admission; 319 distinct remains the preceding evidence.

## Latest authorization: original roadmap M2A and real M2B

Juan explicitly resumed original M milestones and authorized building M2A AND M2B
now. Automated tests/builds/smoke must run as far as possible at block closure;
only manual/user testing waits until the end. Global physical RAM/latency gates
stay documented, but are not a blocker for functional construction or a reason
to restart indefinite optimization. Regex remains deferred without authorization
to add/download/pin any new dependency; do not ask again to continue unrelated M2A.

Active admitted fronts: docs/M2_QUICK_OPEN.md (Ctrl+P project metadata finder,
bounded/cancelable, existing session/encoding/pin) and docs/M2B_NATIVE_WINDOWS.md
(real native Win32/GDI executable, shared Workbench, no editable native buffer).
Only existing workspace crates and system Win32 APIs; no external installs.
Cargo.lock adds jc-gui only, no third-party version change. The existing HTML is
not the GUI app. Use at most three agents total with exclusive files and one
integrator; serialize heavy validation and use broad implementation blocks.
Smoke operates only on the owned HWND/fixture and bounded new report/bitmap paths,
not another application. Do not request a manual test or window approval now.
No push/CI, security/services/other processes/Qwen/GPU changes.

## First real GUI/Quick Open closure; next native selector block active

Read docs/M2_QUICK_OPEN.md and docs/M2B_NATIVE_WINDOWS.md. Ctrl+P project finder
is implemented in TUI: no content scans, lazy bounded worker/latest metadata,
root/query/generation delivery, visible rows, safe labels, existing dirty-session
reuse, side/pin/history and encoding prompt. New opens use FileSession::open_in_root
with a16MiB byte admission and actual-handle confinement. Search Plus delegates
to the moved shared reader; preserve its EOF guard/budgets and fail-closed platforms.

jc-gui is a real Win32/GDI executable with the same Workbench, no child EDIT,
no new external dependencies.339 distinct Rust cases passed in selective closure,
fmt/Clippy/release and16 own-hidden-HWND smoke checks passed. Never infer physical
display/OS clipboard/IME/DPI/accessibility/global RAM acceptance. Smoke reports
paint errors, validates empty/partial clipping and preserves owned fixture bytes.
Save As derives from session.is_new (no untitled mirror); native Save All currently
requires its frontend flow. Ordinary caret placement must not add jump history.
Next admitted block: native GUI Quick Open + Command Palette, reusing core controller
and registry, bounded selector input/visible presentation and completion wakeup.
No dependency installs/regex enabling/CI/push; manual user tests stay for the end.


## Native selectors closure and next broad construction

Read docs/M2B_NATIVE_SELECTORS.md. Native Quick Open/Palette/Open Folder and
--project-root are implemented; one full350 Rust run, Clippy/release and23 native
checks passed. Wakeup supports an optional short notifier outside ticket lock;
Win32 coalesces PostMessage with serialized detach, accepts only in TASKS_MESSAGE,
and painting only prepares bounded rows. Do not repeat passing suites for docs.
Next broad M2B block: OS clipboard plus explicit GUI encoding selection, sharing
Workbench operations and strict encoding. No new dependency/regex/CI, manual user
tests remain deferred. No OS clipboard mutation from the existing native smoke.


## Current three-feature closure and continuation

Read docs/M2_INVISIBLES_MULTILINE.md. Human priority invisible characters + literal
multiline Find/Replace is implemented in BOTH TUI and GUI, preserving clipboard/
encoding work.376 distinct Rust cases in selective closure, fmt/Clippy/release and
29 actual own-HWND checks passed. Do not repeat completed suites/profiles for docs.
ViewToggleInvisibles is one Workbench bool, shared menu/palette/global CtrlAltI.
ASCII diagnostic spans have source byte ranges; never equate scalar/rectangle
columns with display columns, copy indicators or allow CRLF-interior carets.
Text cache896KiB + diagnostic128KiB retains1MiB logical quota; diagnostic caches
invalidate on revision, cold per-line cost remains documented. Display is read-only.

Fields share bounded UTF8 editing/presentation,4096 bytes, exact LF/CRLF/CR and
explicit insert gestures distinct from executing. Literal engine + versioned
apply_text_edits remain the only replace engine/undo. GUI self-posted slots must
give bounded input/paint priority and invalidate only on visible result changes.
Native clipboard calls occur only on explicit actions and outside model borrows;
cut/paste revalidate target document/view/revision/encoding/caret/selection.
Smoke never opens the user's OS clipboard: it uses in-memory transport and pure
UTF16 tests. Rectangular clipboard remains internal, with documented priority.
Encoding selection interprets existing bytes, never silently converts or saves.

The next independent M2B pending flow is native Save All with per-document Save As,
explicit cancellation and no generated untitled writes. Keep one integrator,
exclusive worker files and broad construction before serial validation. Manual
OS clipboard/IME/DPI/accessibility/display remain for the end, no request now.
No new dependency/regex/CI/push or global CPU/RAM profiling.


## Save All closure and next native project-search block

Read docs/M2B_SAVE_ALL.md. Native Save All is implemented: bounded unique document
metadata, per-new-document Save As, no generated-name writes, cancel/error stops
with explicit saved/pending counts. SaveTarget validates id/revision/path/new
after modal calls; ordinary Save As now uses this handoff too. No model borrow
crosses native dialogs; no view/focus/undo cloning.45 directed cases passed,6 new,
382 distinct historical total; fmt/workspace Clippy/release and32 native checks.
Do not repeat complete suites/profiles for docs. Current next functional M2B block:
project Search Plus GUI using existing S1 controller, bounded fields/visible
versioned results, no new engine/permanent index. No dependencies/regex/CI/push;
manual OS-dialog/clipboard/IME/DPI/display gates remain for the end.

## CIDE and native project Search Plus closure

Read docs/CIDE_REBRAND.md and docs/M2B_PROJECT_SEARCH_NATIVE.md. Visible branding
is CIDE; canonical cide/cide-tui/cide-gui retain jc-ide aliases through shared entry
modules. Keep crates, APIs, workspace/repository/remotes and historical evidence.
The supplied JG+CIDE green icon belongs to TUI/CLI; orange variant belongs to GUI.
Seven frames are embedded in all six Windows/MSVC executables; GUI loads group101.

Native project search consumes existing S1, bounded drafts/results,1..4 workers,
two batches per notification, revision/generation/cancel and root invalidation.
validate_context never consumes completions. Version-checked project opening
checks expected source/encoding and confined disk before catalog/focus changes;
do not flatten a live document or remove disk revalidation for a reused disk hit.
391 distinct tests passed across260+131 suites, fmt/Clippy/release and41 native
checks. Raw logs/failures/hashes retained; no new dependency, CI or push. Do not
repeat passing suites or closed profiles for documentation. Project replacement
and diff/status GUI remain pending; manual OS clipboard/IME/DPI/accessibility and
global RAM/display acceptance remain for the end. No user manual request now.

## Native project replacement and read-only Git closure

Read docs/M2B_PROJECT_REPLACE_GIT.md. GUI replacement consumes the S1 preview once:
Enter builds preview, F9/CtrlEnter confirms, one file per TASKS_MESSAGE, Esc drops
remaining plans; open buffers one undo/not saved, closed files confined versioned
FileSession/save. No-op preserves undo/redo/disk; stale encoding/root drops preview.
GUI Git status uses GitStatusController, diff uses existing GitDiffController and
one lazy F3 JobSystem. Explicit session executable/native selector/--git-executable;
only poll_git on task messages accepts completions. validate_context invalidates
metadata without tick/snapshot admission; prepare/draw must not accept or admit.
Status paths never open; hunk navigation revalidates. Preserve provider containment.
53 distinct directed Rust cases and58 native checks passed;8 new/historical399,
not a full399 run. Raw logs and first failed revision-ID oracle are retained.
No new dependency, CI, push or closed profiles repeated. Build broad connected
blocks before integral validation per Juan's nighttime instruction. Next active
M2B front: bounded shared project catalog and native Explorer sidebar contract;
keep typed paths, lazy cancellation/generation, root confinement, dirty/session/
pin/view reuse. Manual OS clipboard/IME/DPI/accessibility/global display remains
for the end; do not request it now or touch Qwen/GPU/RAG/other processes.

The next front has a concrete source-based contract: docs/M2B_EXPLORER_CONTRACT.md.
That inventory is historical; the implementation closure is below.

## Shared Explorer closure

Read docs/M2B_SHARED_EXPLORER.md. ProjectExplorer and ExplorerView in jc-app now
drive both frontends; the old synchronous TUI enumeration/opener was removed.
Keep native opaque paths, generation/revision NodeIds, bounded lazy worker and
global quotas, cancelled-state rejection, dirty/session/pin/view reuse and confined
opening. Windows enumerates by directory handle with ordinary ancestors held
without share-delete; non-Windows metadata fallback retains a namespace window.
Presentation does not poll/admit; GUI mouse targets publish only on WM_PAINT and
validate generation/revision. Compact layout preserves inactive split ownership.
352 related Rust cases,70 own-HWND checks and six own-ConPTY flows passed;
13 new Rust cases,one obsolete old-provider test retired,not a full workspace run.
fmt/Clippy/release passed; raw failures and source/binary hashes are retained.
No new dependency, CI or push. Keep broad construction before integral checks;
manual/global gates remain for the end. Continue pending GUI parity/provider API
per PLAN/EXTENSION_ARCHITECTURE without claiming ABL/LSP/physical acceptance.

## Native navigation closure

Read docs/M2B_NATIVE_NAVIGATION.md. GUI Go to, Back/Forward and search history now
use the existing registry/Workbench. parse_text_position is shared with TUI;
positions are one-based source scalars at input, not display/LSP coordinates.
Modal input64B captures document/revision/encoding/ViewId/pane; accept/edit/paste
revalidate. cancel_search_navigation disarms deferred Find movement without
stopping the scan. Search history remains in Workbench; palette moves the single
Find/Replace draft and restores it for cancel/history/options, preserving replacement.
Keep PaneManager pin semantics: automatic Go to routes to the other group, while
Back/Forward stays in the active group's history. Do not change that contract to
match the initial failed oracle. Menus gate availability before closing overlays.
78 directed Rust cases and14 own-HWND section-navigation checks passed (4/7 new);
fmt/Clippy/release passed. No full workspace/native run, Explorer suites or closed
profiles repeated. Raw failures/hashes retained; manual/global gates remain open.
Next pending GUI parity is universal line/comment commands already in jc-app and
language metadata. Build connected blocks before tests; no new providers/AVM/CI/push.

## Native line/comment editing closure

Read docs/M2B_NATIVE_LINE_EDIT.md. GUI Duplicate/DeleteLine/MoveLineUp/Down and
ToggleLine/BlockComment now route to the existing Workbench and profile metadata.
CommandContext.editor_input plus Editor focus/linear selection governs these six;
GUI modal/read-model focus cannot edit hidden source. Palette also revalidates
document/revision/encoding/ViewId/pane/caret/anchor from its invoker. Keep all byte
edits, undo and view rebasing in Workbench, with read-only presentation.
Delete/Move operate on caret line, Duplicate uses a selection when present. Preserve
existing EOF-empty-line semantics; do not invent multi-line movement or guaranteed
terminal-EOL preservation for explicit line transforms. No ABL frontend branches.
23 directed Rust cases and14 own-HWND line-edit checks passed first run (3/7 new),
fmt/Clippy/release approved. No navigation/Explorer suites or closed profiles rerun.
Continue remaining universal GUI parity before optional provider APIs. Manual/global
gates, omitted optional symlink and two missing GNU notices remain open. No new
provider/AVM/dependency/install, CI, push, publication or intermediate manual request.

## Selection search and reopen closure

Read docs/M2B_SELECTION_REOPEN.md. GUI selection next/previous, ReopenClosed,
SearchCancel and Tab1…9 share registry/Workbench. Source selection requires Editor
input/linear selection and current palette target; cap both raw range and decoded
UTF8 before query mutation. Complete cached queries may navigate immediately;
deferred-delivery tests must force a new Running scan before editing the source.
try_reopen_closed propagates errors without consuming closed metadata; old bool
API delegates. Already-open exact/canonical paths fail without another buffer or
dirty reload. Reopen uses current disk or blank new-file, sameTabId/newDocumentId,
no discarded draft/undo recovery. close_flow owns the sole native confirmation
route and revalidates source intention; F1 still decides view vs last document.
11 directed cases and15 own-HWND checks passed (4/8 new), fmt/Clippy/release approved.
Retain setup/compile fixture failures and hashes; no closed suites/profiles rerun.
Continue remaining universal GUI selection/rectangle parity under existing scope;
manual/global gates, omitted symlink and GNU notices remain open. No new provider,
AVM/dependency/install/security changes, CI, push or publication.

## Native rectangle/grid closure

Read docs/M2B_RECTANGLE_GUI.md and the limited repertoire contract. Six rectangle
commands and Alt+drag now use Workbench/VisualPosition/LineLayout, not a GUI engine.
Rectangles override diagnostic escapes with one TAB4 grid in both frontends;
invisibles marks cells without changing global preference. Preserve one-action
editing, bounded internal rectangular clipboard, no implicit OS rectangular format.
Palette/drag targets include visual anchor/head because byte offsets may not move.
SetCapture/ReleaseCapture happen outside model borrows; release only own HWND.
Capture loss/resize keeps last admitted selection, Escape clears it, stale cannot
revive it. MouseMove without drag does not continuously repaint. Normal arrows
exit rectangle even at BOF/EOF; editor_input also protects TUI modal controls.
26 directed cases and15 own-HWND checks passed (4/8 new), fmt/Clippy/release approved.
Raw focus/edge/compile/lint failures and hashes retained, no closed profiles rerun.
Finish the remaining universal GUI inventory before optional provider APIs; global/
physical gates, general Unicode columns, OS clipboard, symlink and GNU notices stay
open. No new dependency/provider/AVM/install/security changes, CI, push or publication.

## GUI command inventory and edit/group closure

Read docs/M2B_GUI_INVENTORY.md, its 104-row command matrix and static source hashes.
85 commands have actual GUI routes; 19 lack direct support for documented reasons.
Palette lists unsupported commands disabled; shared bindings are not proof of modal
dispatch or physical keyboard acceptance. Native Menu* adaptation is partial.
Indent/Outdent now have Edit entries; ScrollLink has View/keyboard/palette and a
persistent indicator. Indent/Outdent/Join/Split require actual editor_input in both
frontends. Preserve Workbench delta-only linked scrolling, independent pin/carets,
strict encoding, one undo and no source edits behind modals/Explorer/stale palette.
Two directed Rust cases and 12 own-HWND checks passed (one/five new), fmt/Clippy/
release; initial fixture lint retained. No closed suites/profiles repeated.
Next concrete universal integration is GUI generic bottom-panel host, existing F3
Problems/Output and three-region focus. Define row/destination/state/cancellation
contract first, without inferring new provider implementation. Project results are
a specific panel, not that generic host. Preserve outstanding selector input,
native menu, physical/global/Unicode/clipboard/symlink/notices limitations.
No new dependencies/providers/AVM/install/security changes, CI, push or publication.

## GUI F3 panel host closure

Read docs/M2B_BOTTOM_PANELS.md. Workbench TaskPanels now owns typed bounded rows,
stable IDs and stamp/path/range provenance, replacing the prior string logs/TUI
completion formatting. GUI has one F3 jobs.poll dispatcher inside TASKS_MESSAGE:
Git controller first, then accept_task_completion; no polling/admission in paint.
JobRecord and CompletedTask capture path at admission; SaveAs invalidates results
even at unchanged revision/encoding. Output provenance is not navigation authority.
Problems activation by ID revalidates open document/stamp/path/view/both source
boundaries before existing jump/pin/history logic. Never reopen disk/apply proposals.
ViewToggleBottomPanel/Problems/Output, three-region F6/reverse focus and Escape
are connected. Hiding/reading never starts or cancels providers. Root change cancels
existing jobs/resets logs; stale completion may log only generic discard status.
Keep quota accounting including row/catalog/string/path capacities, one text owner,
visible128rows/512columns, painted hit IDs and compact fallback. Four new shared
tests,23 directed cases and13 own-HWND checks passed (six new native),fmt/Clippy/
release. Native fixture providers are synthetic only; no business/AVM validation.
88 GUI routes now; historical inventory artifacts remain unchanged. Remaining
universal inventory gaps include selector input, menu adaptation and general mouse.
Preserve physical/global/Unicode/clipboard/symlink/notices gates. No new dependency,
provider, AVM, install/security changes, CI, push or publication.

## Native selector input closure

Read docs/M2B_SELECTOR_INPUT.md. QuickOpen/Palette/Encoding now share UTF8 byte
FieldSelection/edit_selected_field and the bounded presentation walker. Preserve
256B single-line policy, atomic replacement quota/control/bidi rejection and
separate field caret/anchor from candidate ordinal. Home/End edit caret; CtrlHome/
End choose first/last, UpDown/Page/TabShiftTab navigate results. No source undo.
Native Edit/keyboard CutCopyPasteSelectAll use entry_context.field_input; palette
command rows still use their invoker's source context. Do not conflate these routes.
Selector clipboard runs outside model borrows, checks generation/input intention/
candidate/source stamp/path/view/invoker/root/focus/encoding target before and after.
Keep destination UTF8 decoder preflight256B and existing strict native2MiB allocation
guard. Tests use ONLY synthetic smoke transport, never the private OS clipboard.
Action boxes that handoff metadata only; no large inline payload on every event.
Escape preserves search draft/Explorer/bottom invoker, Files/Encoding acceptance
returns Editor. Three new field tests and one decoder test;23 distinct directed
cases passed (24 pass lines include one filter overlap),13 own-HWND checks with
six new,fmt/Clippy/release. Initial fixture/private-getter/lint failures retained.
No closed suites/profiles repeated for docs. Inventory remains88 GUI routes;
native menu adaptation, linear drag/wheel and physical/global/Unicode/clipboard/
symlink/notices gates remain open. No new dependency/provider/AVM/regex/install/
security changes, CI, push or publication.

## Native menu and source pointer closure

Read docs/M2B_NATIVE_INTERACTION.md. NativeMenuGroup composes shared menu/registry
and real GUI routes, with no numeric category ranges or duplicate business IDs.
Six Menu* macros and AltF/E/S/P/T/V/N/G/F10 open actual HMENU; Build/Help remain
unsupported. Labels/checks reflect current focus/capability. MenuStamp captures
source/field/root/focus/modal generations/Explorer selection; delayed choices
revalidate before closing modals or acting. Never borrow State across popup/capture.
Palette menu macros restore their Find invoker draft; no separate HWND case for
that combination yet. Keep selector field versus palette-invoker routing distinct.

Linear drag/Shift retains original pane/anchor; hits explicitly map current
scalar/diagnostic/TAB4 projection to legal bytes. No autoscroll or word selection.
PointerContext rejects stale source/path/root/focus/invisibles for both drags.
Wheel accumulates signed120 fractions per source context under pointer, preserving
focus/carets/selection; source OS screen conversion/settings are outside borrow.
ViewState.manual_scroll suspends reveal until explicit movement/edit/selection;
resize clamps rows, linked scroll applies consumed delta, pin stays independent.
Retain one editing model: old paint::click and numeric create_menu are removed.

16 distinct directed Rust cases/four new and13 own-HWND checks/six new passed,
fmt/Clippy/release. Build complete before tests; initial three smoke style lints
retained, native passed first run. Current94 GUI routes/104 historical IDs is not
full/physical acceptance. Preserve physical/global/Unicode/clipboard/IME/DPI/
accessibility, horizontal/autoscroll/word-selection/field mouse, symlink/notices
gates. No closed suites/profiles repeated for docs. Keep broad construction before
tests, manual/OS at end. No new dependency/provider/AVM/regex/install/security
changes, CI, push or publication.

## Menu-invoker validation follow-up and admission boundary

Read docs/M2B_MENU_INVOKER_VALIDATION.md. It closes the previously missing
Palette->Menu*->Find/Replace case at171af05: six macros, three active-field states,
36 base cancellations, continued field input, permitted View action, rejected
source edit and eight stale field reentries. One new integrated check plus seven
startup/paint checks passed first run, three CLI rejections, fmt/ClippyGUI/releaseGUI.
Zero unit test runs or repeated historical runtime suites; product handlers unchanged.
The earlier no-separate-case statement is historical, superseded by this evidence.

The scope audit found horizontal mouse/autoscroll/word-selection/field mouse only
as limitations or exclusions, not admitted construction. Do not convert their
pending status or broad M2B acceptance into a build order. A next increment for
those gestures requires an explicit scope/coordinate/limits/cancellation decision;
do not add plan scope retrospectively to justify work or infer M3/providers.
Preserve all physical/global/Unicode/OS clipboard/IME/DPI/accessibility/symlink/
notices gates. No new dependency/provider/AVM/install/security/CI/push/publication.

## Cierre local — resaltado compartido, 2026-11-03

Por prioridad explícita de Juan se integró resaltado léxico ABL, Rust, JSON y
JSONC en TUI y GUI; otros perfiles permanecen Plain. Contrato, límites, fallos
iniciales y evidencia: `docs/M2_SYNTAX_HIGHLIGHTING.md`.
Pasaron35 casos Rust distintos (18 nuevos),11 checks HWND (4 nuevos),5 flujos
ConPTY,3 rechazos CLI, fmt, Clippy y build release locales offline. No se ejecutó
toda la suite runtime del workspace ni CI. No equivale a aceptación física/global.

El diff entre documentos abiertos queda suspendido y recuperable en
`docs/checkpoints/open-diff-before-syntax-66aa783.zip` y su manifiesto; no está activo
ni validado. No restaurarlo automáticamente sobre el resaltado.

La autorización posterior del usuario permite incorporar y fijar una dependencia
Rust mantenida para regex real (candidato regex-automata) en el siguiente bloque
Search Plus. Sustituye la restricción anterior para esta dependencia concreta:
no pedir de nuevo permiso. No se añadió en este bloque, regex sigue deshabilitado
y no están autorizadas instalaciones de programas del sistema.

Preserve one revision/encoding/path/profile-aware SyntaxCache per document,
shared across views. Lexer metadata belongs in jc-languages; bounded cache and
palette in jc-app; rendering consumes prepared rows only. Continue deep pending
work in owner events via continue_syntax, independently of exposure paints.
64KiB is per-call work,128KiB is logical syntax cache/document; text layout768KiB
plus invisibles128KiB preserves the1MiB total cache budget. Honor NO_COLOR.

## Regex implementado — continuación TUI, 2026-11-03

Regex real de documento y proyecto está integrado con regex-automata=0.4.18,
autorizado por Juan. Alt+R, historial, rangos originales, cancelación/cuotas y
reemplazo literal por transacciones; `docs/M2_REGEX_SEARCH.md`.
Esta actualización sustituye las menciones históricas de regex deshabilitado.
124 casos distintos aprobados observados, cuatro flujos ConPTY y fmt/Clippy/
release locales. Dos fixtures junction y seis Git fallaron por creación/acceso
restringido: no se declara suite global aprobada ni aceptación física/RAM.

La instrucción vigente de Juan es continuar hasta terminar los pendientes TUI,
en bloques amplios, validar al cerrar cada bloque y activar el siguiente sin
esperar otro «continúa». Siguiente bloque: comparación de documentos abiertos
sin Git, desde el checkpoint revisado sobre el código actual. No restaurar fuentes
antiguas sobre resaltado/regex. Proveedores ABL/AVM y aceptación física conservan
sus requisitos reales; no afirmar capacidades que solo tengan contrato.

Regex must remain cooperative with charged retries, no whole-document decoded
copy or unbounded backtracking. Reuse compiled patterns across revisions and
share them across file workers. Regex replacement is literal; document the
simple-fold/unsupported Unicode-boundary distinction from literal full folding.
Do not repeat dependency permission. Keep CI dormant and validate broad blocks.

## Comparación de abiertos TUI implementada — 2026-11-03

El checkpoint suspendido de comparación sin Git ya se integró sobre regex y
resaltado actuales: controller con dos orígenes versionados, admisión dual F3,
presentación compartida acotada y visor TUI lateral/unificado. Comandos View/
paleta, hunks, navegación a grupo original, stale/refresh/cancel; fuente de solo
lectura y sin writes. `docs/M2_OPEN_DOCUMENT_DIFF.md`.
GUI de este visor sigue pendiente; no restaurar su checkpoint automáticamente.

54 casos distintos observados aprobados, cuatro flujos ConPTY, fmt/Clippy/release.
Las nueve regresiones Git pasaron con acceso autorizado a fixtures propios tras
fallar por acceso sandbox; no se usa Git en la comparación del producto.
No CI ni aceptación física/global. Siguiente bloque realmente activo: contrato
de capacidades internas opcionales y consumidor TUI de símbolos/navegación,
según EXTENSION_ARCHITECTURE y `docs/M2_TUI_FINISH.md`.

Preserve dual-origin validation for OpenDiff, one flight plus latest metadata,
combined snapshot/artifact reservations, read-only bounded OpenDiffView and no
admission/poll from preparation/draw. Controller owns accepted immutable result.
Escape routes through DiffCancelOpen; original-group hunk navigation respects
ViewId and byte boundaries. GUI capability remains disabled until wired.

## Capacidades locales de fuente TUI — 2026-11-03

Primer registro interno namespaced y proveedor opcional progress.abl operativo
para símbolos y referencias léxicas; core.words completa palabras textuales de
cualquier documento. Listas/selector TUI, entrega F3, stale, candidatos confinados
y propuestas de completado por revisión/una transacción. `docs/M2_SOURCE_SERVICES.md`.
40 casos distintos (17 nuevos), cuatro flujos ConPTY, fmt/Clippy/release locales
aprobados. No AVM, semántica, loader externo, GUI ni aceptación física/global.

Siguiente bloque activo: plantillas/snippets universales y catálogo ABL del
proveedor opcional, parámetros/preview/aceptación y undo único. Continuar sin
esperar otro «continúa»; construir el conjunto antes de su ventana de validación.

SourceRegistry/SourceController and SourceItem are generic jc-app contracts.
ABL lexical services live only in optional jc-progress/source-services. F3 owns
bounded snapshots/results; only explicit activation opens paths/applies edits.
Never inherit the origin encoding into a different target document. Preserve
dirty/pin/document identity and candidate ambiguity. Completion is textual words
before the caret, not semantic completion. No new provider processes at startup.

## Snippets TUI — cierre local 2026-11-03

Motor universal acotado con parámetros, preview y aceptación por revisión/una
transacción. progress.abl aporta siete plantillas declarativas de Insert/Surround;
sin reglas ABL en jc-app. Preserva encoding/BOM/EOL/undo; propuestas obsoletas se
rechazan. `docs/M2_SNIPPETS.md`:38 casos distintos (11 nuevos), cuatro
recorridos ConPTY, fmt/Clippy/release aprobados. Sin AVM ni aceptación física/global.
Sigue auditoría final de teclado, foco y flujos TUI; CI permanece en M5.

Snippet definitions belong to provider metadata; generic SnippetSession owns a bounded proposal, validates the exact origin and applies one expected-revision transaction. Never interpolate recursively or run generated ABL during editing.

## TUI overlay closure, 2026-11-03
Topmost overlay owns both keys and Event::Paste, even above snippet/source/diff.
Prompt/path4096 UTF8, position64, palette256; input limits must be atomic.
117 distinct TUI cases plus three ConPTY flows passed; physical clipboard/global
acceptance remains open. Next active block: explicit Windows clipboard in TUI.
