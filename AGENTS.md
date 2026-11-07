# Extreme GUI geometry — 2026-11-07

Read docs/M2_GUI_EXTREME_GEOMETRY.md. The prior buffer rejected 4096×2049 and
5120×2160 on an owned hidden HWND. Oversized frames now share one DIB capped at
32 MiB across horizontal bands. Keep client coordinates, invalid-region clipping,
reuse across bands and only publish a band after its draw completes. Eight hidden
release cases passed, including synthetic DPI 96/144/192, seams and return to base.
35.23 MiB private at 5120×2160 with a 16 KiB probe target. Full-frame p95 24.12 ms
with a separate full-size probe DIB. Four directed buffer tests, fmt, GUI all-target
Clippy and workspace release offline passed. Delivery target/m2-geometry-next/release.
Physical acceptance remains open. Preserve prior releases and caches.

# Combined editor services performance — 2026-11-06

Read docs/M2_EDITOR_SERVICES_PERFORMANCE.md. Three serial release runs of one
owned hidden GUI exercised Search Plus, embedded Git status, Markdown image and
private periodic recovery. 360 native compositions, 128 during active recovery
and 29 with both queries pending and pixels retained. Peak private 27.08 MiB
includes 2.48 MiB probe DIB. p95 3.72–4.05 ms. Three byte-exact checkpoint restores
and unchanged source/Git hashes. CLI Markdown paths now resolve against the
canonical document directory fixed at preview admission. Seven directed media
tests, fmt, GUI all-target Clippy and workspace release offline passed. Current
delivery target/m2-services-next/release. Keep prior deliveries and caches.
Extreme geometry and physical acceptance remain open. No desktop or personal depot.

# Complete-editor process performance — 2026-11-06

Read docs/M2_EDITOR_PROCESS_PERFORMANCE.md before extending performance probes.
Owned hidden GUI mode --smoke-performance-only uses in-process WM_PRINTCLIENT,
one retained production DIB and a separately reported2,600,000B probe DIB.
949 native compositions/160edit-undo cycles passed; five real GUI processes.
Peak private46.00MiB for8x4MiB visited docs,6.04MiB afterclose/probe release;
working set57.91MiB is a different metric. Three owned TUI ConPTY processes,
189statusVT samples/24edit-undo cycles; peak17.95MiB. TUI edits ASCII marker,
not long/Unicode line; parser corpus excludes wide/combining. No physical claim.
Markdown scene replacement cancels accepted pixels without waiting for taskpoll;
empty cancellation is idempotent, worker stays lazy/asleep without UIjoin.
Six directed GUI cases passed, one new; fmt/GUI all-target Clippy/workspace
release offline passed. Reuse both caches, current binaries target/m2-process-next/release.
No full runtime/HWND suites, desktop/clipboard/personal recovery, dependencies,
CI orpush. Preserve prior deliveries. Base50MiB goal observed locally at1000x650,
not global acceptance: combined optional services/extreme geometry/physical gates
remain open. IME excluded, Narrator deferred. Do not act on old window IDs.

# Current blame closure — 2026-11-06

Read docs/G3_BLAME.md before extending line attribution. Independent Rust core,
one existing DiffModel, first-parent/exact-path pinned sources; no live-buffer blame.
128rows/page,128walked,2MiB/blob,4MiB/pair,16MiB cumulative blob reads; retained base
plus pair can total6MiB source payload. Explicit unknown origins at shallow/work/read
boundaries; missing/corrupt advertised objects remain errors. One revision worker,
poll-only acceptance; same-path responsible comparison retains page for return.
GUI/TUI connected.11 new directed cases passed by suites; fmt/selective all-target
Clippy/workspace release offline passed. Binaries target/blame-next/release; preserve prior.
No new full tests/HWND/ConPTY/desktop, dependencies/lock, CI or push. User is working:
do not take desktop. Physical M2 unchanged; IME excluded, Narrator deferred.

# Current per-file history / outline closure — 2026-11-06

Read docs/G3_PATH_HISTORY_OUTLINE.md before extending per-path Git or outline.
Independent jc-git path_history:24matches/128walked, private repository/start/path
cursor, explicit shallow/metadata clipping, no HEAD resolution/worktree/blob reads
for listing or rename-following. One existing revision worker and poll-only acceptance.
GUI/TUI connected; accepted page retained only for same-path membercommit comparison.
GUI summary stays pinned to compared commit while browsing; failed comparison visible.
Outline has explicit close, bottom Esquema reopen and navigation; cancels only own jobs.
13 new directed cases passed by suites, fmt/selective all-target Clippy/workspace release
offline passed. Binaries target/path-history-next/release; preserve previous release paths.
No new full tests/HWND/ConPTY/desktop, dependencies/lock, CI or push. User is working:
do not take desktop. Physical M2 gates unchanged; IME excluded, Narrator deferred.

# Current Save All correction — 2026-11-06

Read docs/SAVE_ALL_NEW_PUBLICATION.md before changing new-file saving.
Windows create-only uses MoveFileExW WRITE_THROUGH without REPLACE_EXISTING/COPY_ALLOWED,
after existing flush/sync/close. Hardlink WinError5 reproduced in owned workspace probe.
Existing-file ReplaceFileW/conflict/recovery paths unchanged; no overwrite/copy fallback.
3 new file regressions +1 parser test and3 directed SaveAll HWND flows passed (8 common
fixture checks also passed). No desktop/Git/full suites repeated. Fmt/selective all-target
Clippy/workspace release offline passed; binaries target/save-all-next/release, prior paths
preserved. The previous specific SaveAll blocker is fixed, not full M2 acceptance.

# Current Git visual redesign — 2026-11-06

Read docs/G2_GIT_VISUAL_REDESIGN.md before changing Git presentation.
Approved compact native design implemented; current binaries target/git-redesign-next/release.
112 distinct GUI/input cases by suites/directed closure,25 directed own-HWND Git checks,
fmt/workspace all-target Clippy/release offline passed. Full HWND suite stopped before
Git on Save All access denied; reproduced with prior G2 binary, not a passing full run.
Real GDI image/evidence in docs/evidence; visible final review interrupted by user Escape.
Do not take desktop while Juan works; final owned instance normal exit unconfirmed.
No new Git capabilities/TUI/core/dependencies/recovery changes. Show result before G3.

# Working on JC-IDE

## Markdown images / targets / nesting — 2026-11-06

Read docs/M2_MARKDOWN_MEDIA_LINKS.md. Shared span targets and hanging indent
retain source ranges/bytes. AnchorScan uses64lines/256KiB perstep,4MiB/65536lines
total, revision/view/codec validated; no permanent index or direct edit. GUI
links publish after paint and require current scene/stamp/root/DPI/viewport.
Local links open through Workbench; web HTTP/S only by explicit action. GUI
image controller has one lazy worker and poll-only accepted batch. Local reads
reuse jc-fs; remote load explicit with WinHTTP, no automatic downloads. Preserve
encoded8MiB/decoded16MiB/4096dim/4Mpixel/16resources bounds and64extent metadata
to prevent reflow/reload loops. TUI keeps alt text and keyboard link navigation.
33 directed cases passed by affected suites,20new; no full workspace runtime run.
Loopback WinHTTP final passed outside restricted environment; no Internet test.
No desktop/clipboard/personal recovery/CI/push; prior physical gates unchanged.
Deliveries target/markdown-media-next/release; preserve earlier paths and caches.


## Native Markdown presentation — 2026-11-06

Read docs/M2_MARKDOWN_RENDERED_VIEW.md. Shared MarkdownRow block/continuation and
MarkdownSpan style preserve source/range quotas; GUI consumes native fonts and
geometry, TUI keeps spans and menu check. ViewMarkdownPreview is gated by detected
Markdown profile, not source starting with #. No second document or web renderer.
13 directed distinct cases passed,6 new; selected all-target Clippy passed. Current
binaries target/markdown-render-final/release; preserve older deliveries/caches.
Native raster in docs/evidence is own GDI painter output, not physical screenshot.
User requested Computer Use; fresh target selection/source accessibility worked,
but black capture and GetCursorPos accessdenied repeated after fresh selection.
No input sent; physical review and own fixture instance normalexit pending until
usable desktop. Never reuse recorded window IDs/PIDs or forceclose. Personal
recovery disabled and fixture bytes unchanged. CI/push remain dormant.


## M2 menu field paste closure — 2026-11-06

Read docs/M2_MENU_FIELD_PASTE.md. GUI field_paste_action is the shared visible-field
intention for command availability/execution/menu; do not close that field first.
Selector clipboard remains separate; readonly results/previews never redirect to
the editor behind them. TUI menu availability may consult underlying Git/snippet
budgets, but CtrlV/Event::Paste cannot cross the menu or access transport.
20 directed cases passed by suites,8 new (6 GUI,2 TUI); fmt, GUI/TUI all-target
Clippy and workspace offline release passed. No desktop/clipboard or
recovery personal/CI/push. Current binaries target/m2-field-paste-next/release;
reuse existing caches and preserve earlier deliveries. Physical/global M2 gates
remain open; IME excluded/Narrator deferred. User authorized local integration
of completed branches into main; only one worktree existed at start of this block.


## Commit search / build cache closure — 2026-11-06

Read docs/G3_COMMIT_SEARCH.md and docs/LOCAL_BUILD_CACHE_CLEANUP.md.
Shared jc-git literal query covers full messages, author/email, SHA prefix;
first-parent only, pinned start,24 matches/128 walked per page, bound cursor.
Use existing one revision worker; poll alone accepts. Empty pages may continue.
GUI CtrlF/TUI F inside Git opens bounded256B grapheme field; edits cancel stale
results. Opening member revisions retains one result page; no editor-source writes.
11 directed new cases passed by suites (5 core,2 app,2 GUI,2 TUI). Fmt, selected
all-target Clippy and workspace offline release passed; no desktop,
HWND/ConPTY/personal clipboard or recovery/CI/push. No new dependencies.
Current deliveries target/commit-search-next/release; preserve earlier binaries.
User authorized cleanup:244 Cargo intermediate directories removed,15.74GiB logical;
340 protected binaries verified unchanged. Sources/evidence/toolchain/recovery safe.
Use scripts/build-cide-local.py --output target/<new-delivery>/release for release;
stable target/build-cache, export only verified binaries. Reuse embedded-git-next
for directed debug checks; serialize heavy validation. Do not create fresh dependency
caches per block. IME excluded/Narrator deferred; global M2 acceptance unchanged.


## Git layout / parallel projection closure — 2026-11-06

Read docs/G2_GIT_LAYOUT_COMPARISON.md before extending native Git presentation.
GUI layout supports One/TwoFilesLeft/TwoFilesRight/Three with DPI minimums,
versioned split/rail capture and compact tabs. GUI metadata v5 appends7 validated
preference bytes; reader retains v1–v4. Recovery sources/generations unchanged.
One bounded row projector serves buffer/history/local; no second diff engine,
source I/O or completion acceptance in prepare.128logicalrows/256parallelcells,
64KiB source/cell,4096textbytes/512visiblecols,16384horizontalcols; limited row
warns at start and drops potentially incomplete trailing grapheme. Page/extent
metadata use visible rows; offset normalization performs at most one extra pass.
107 GUI/input cases passed (101+6),15new;26directed closure after glyph/viewport
fix. Fmt/Clippy all-target/release offline and147 native checks passed; no fresh
full workspace runtime or physical/ConPTY claim. Hidden-HWND fixture settles first
paint before comparing editor-view invariants; initial failed harness logs retained.
Current binaries target/g2-layout-next/release; preserve all prior release paths.
No TUI/core Git/dependency change. G2 remaining tree/file tabs and G3 tools stay
separate from M2 physical acceptance. Prior desktop normalexit record remains
pending until fresh Computer Use selection; do not assert window still exists
without checking. IME excluded, Narrator deferred, crossmonitor unverified. CI dormant.

## Local Git pairs / independent filesystem closure — 2026-11-06

Read docs/G1_LOCAL_SOURCE_PAIRS.md before extending Git source selection.
jc-git::local_pair reads HEAD/index and index/raw-disk sources with typed absence,
exact parsed-index SHA256/checksum, bounded handle captures and final revalidation.
jc-fs is internal std-only; jc-files retains compatibility exports. No editor
crate dependency in Git, no duplicate confined reader, no external package added.
GitRevisionController shares one worker/queue for historical/local queries; only
poll accepts. Source mode is explicit, buffer comparisons retain their own route.
GUI/TUI show presence/mode; no live-buffer hunk navigation for local/history.
744 distinct semantic cases passed by suites/directed closure,8new, not a final
single744 workspace run. Original fixture/message failures retained and corrected.
Fmt/Clippy all-target/release offline and144 HWND checks passed; no new physical
or ConPTY claim. Current binaries target/m2-git-pairs-next/release; preserve prior.
Fresh desktop capture remained black, no input; previous owned history GUI still
needs normalexit. See work/g1-local-pairs-desktop.json. No forceclose or personal
recovery/clipboard use. M2 final interactive acceptance waits usable desktop;
IME excluded, Narrator deferred, secondmonitor unverified. G2/G3 stay separate
expansion, writes/network M5. Do not repeat closed suites for documentation.

## Historical Git / reusable diff closure — 2026-11-06

Read docs/G1_HISTORICAL_READS.md. jc-git::revision reads pinned first-parent
revision lists/pairs; absence is typed, missing/corrupt advertised objects are
errors. read_file_pair never reads worktree sources. Tree bytes cumulative8MiB,
depth128,entries65536,parentmetadata8; host source2MiB/side and4MiBcombined.
The single diff model now lives in jc-git::diff_model; jc-app only reexports.
GitRevisionController owns one lazy bounded worker/latestmetadata/completion;
only poll accepts, prepare does no I/O/admission. GUI commit/file selection and
TUI H/Intro/E/Esc show historical sources without editable-buffer navigation.
No historical side tab, complete DAG, refs or correct HEAD/index/disk pairs yet.
Current binaries target/m2-history-next/release. 734 initial workspace cases,
335 revalidation after modelmove +2 additional cases =>736 distinct closure,
not one final736 run. Clippy/release,142 HWND and7 ConPTY passed. Visible test
blocked by GetCursorPos accessdenied/black capture; own instance normalexit pending.
See work/g1-history-desktop-result.json and fresh Computer Use selection before
closing. Source/Git fixture unchanged; personal recovery off. IME excluded,
Narrator deferred, physical/global gates open. CI dormant; no new dependencies.

## Shared Git core boundary — decision by Juan

Read docs/GIT_SHARED_CORE.md and D63. The embedded Rust Git core is reusable by
CIDE, OmniCore and OmniCoder. Keep one implementation, independent of jc-app,
editor types and GUI/TUI. Host adapters own document/task/UI integration.
Do not create bindings or assume the other hosts are integrated without inspecting
and validating them. Full G1–G3 expansion does not all block M2 closure; writes
and network stay in M5. No repo split/rename needed for this documentation decision.

## Git expansion research / REA engine — 2026-11-05

Read docs/SUBLIME_MERGE_REA_STUDY.md, docs/GIT_CAPABILITIES_CIDE_AUDIT.md and
docs/REA_GHIDRA_SETUP.md for the admitted G1–G6 scope and installed tools.
Juan authorized configuring Ghidra12.1.4/JDK21 for REA. Scoped doctor11 checks,
real native search, three traces and three decompilations passed; session closed
and source unchanged. One330s startup timeout is retained; use configured900s
startup/1100s MCP allowance, no analyzers disabled. Their private runtime is outside
the repo; no new editor dependency or shipped proprietary resource. Current
editor binaries remain target/m2-git-ui-next/release. G1 requires explicit
Buffer/Index/Worktree/Commit/Absent source identities before historical UI.
G4–G6 writes/network stay in M5. Native query evidence is distinct from resource
declarations; do not claim recovered original source or all-tools parity.
Do not repeat Cargo suites for this documentation/tooling-only block.

## Current native Git workspace / themed picker closure — 2026-11-05

Read docs/M2_GIT_WORKSPACE_NATIVE.md before changing Git presentation or pickers.
Current binaries target/m2-git-ui-next/release; preserve prior paths. Git uses an
independent native pixel workspace, not the project-search painter. Keep one diff
viewport and paint-published versioned hits. History is first-parent, max24 commits,
metadata max24KiB charged against status quota; no complete branch graph or historic
file diff claim. No Git writes/network in M2. Ordinary absent .git is NotRepository;
invalid present metadata remains an error. Own themed folder/file modal uses the
shared lazy explorer, no directory I/O in paint or Workbench borrow across modal.
718 cases passed by suites/directed closure,139 native checks, visible picker/Git
clicks/details and normal exit; no single full runtime pass. IME excluded, Narrator
deferred, one display only; global physical latency/RAM open. CI remains dormant.

## Current selection history / M2 input closure — 2026-11-05

Read docs/M2_SELECTION_HISTORY.md before changing editing/history/recovery.
Juan excludes real IME from the requested M2 closure; no fonts/languages install.
Narrator stays deferred. Windows WMI reports one active physical display.
Selection metadata lives only in jc-app, bounded1024 records/256KiB per document,
lazy geometric reservation before editing, allocation-free record afterward.
One logical operation must publish one core transition; nested commands share
the selection scope. Core history_transitions is a borrowed ordered iterator,
not another history or a source snapshot. Undo/redo restore only the originating
ViewId without changing focus; closed origin or changed codec uses F1 mapping.
CIDEAPP2 reads APP1 with empty metadata; CIDEDOC1/CIDEPAN1 unchanged. Remap ViewIds
through the pane recovery mapping and validate revision/length/view ownership.
Preserve periodic metadata/identity/epoch, HEAD/BACKUP and handle-based GC.
CtrlHome/End and CtrlShiftHome/End are shared editor-only commands, palette-only
without menu clutter, endpoints after BOM/EOF without full decode. Insertion
clears collapsed linear anchors. Current binaries: target/m2-selection-next/release.
708 full-workspace tests passed, three helpers through parents; fmt/all-target
Clippy/release offline passed. 139 own-HWND checks,9 private ConPTY input checks,
GUI normal+periodic recovery and TUI periodic recovery passed. ConPTY BS parsing
was fixed in the common test reader, not production. Do not repeat closed suites
for docs. IME excluded, Narrator deferred, physical cross-monitor not verified;
screen/input observations and remaining limits are in VALIDACION. CI stays dormant.
The first desktop trial verified real typing, oriented selection Undo/Redo and
normal AltF4. A final isolated instance with ShowNormal displayed the native UI,
then Windows input failed again with GetCursorPos access denied. Normal close
is pending; see work/m2-selection-desktop-final.json and launch report. Select
fresh returned windows through Computer Use, never act from stale handles/PIDs
or force another app/process to bypass desktop restrictions. Source is unchanged,
personal recovery disabled. Retain the explicit IME exclusion in future closures.

## Current sidebar navigation closure — 2026-11-05

Read docs/M2_SIDEBAR_NAVIGATION.md before changing sidebar input. ExplorerView
owns viewport separately from selection: scroll_by/to never move the node;
select/keyboard movement reveal it again. Presentation.first participates in
GUI/TUI hit/drag identities. Keep wheel focus/caret unchanged, bounded128 rows,
catalog generation/revision checks and native capture ownership. GUI/TUI current
binaries are target/m2-sidebar-next/release; preserve previous paths. Seven new
regressions,254 selective tests,138 own-HWND checks and7 private ConPTY checks
passed. One full workspace Rust1.88 runtime run passed683 tests, three helpers
invoked by parents; fmt/workspace all-target Clippy/release passed. Logs in
VALIDACION.md; no physical acceptance. Do not repeat suites for docs-only changes.
Git embedded block is committed locally as223b050. Keep CI dormant. Narrator stays
deferred; real IME/second-monitor availability must be confirmed, never inferred
from synthetic DPI/IME contracts. Exact historical selection restoration remains
the F1 UX improvement: no second content undo or unbounded view snapshots.

## Current embedded Rust Git closure — 2026-11-05

Read docs/GIT_EMBEDDED_ENGINE.md and docs/GIT_EMBEDDED_LICENSES.md first for Git.
Juan authorized gitoxide adaptation and its dependencies. jc-git pins gix0.90.0;
Rust1.88.0 is installed only in work/toolchain and fixed by rust-toolchain.toml.
GUI/TUI production state and HEAD/index blobs no longer execute/discover git.exe.
Do not restore that fallback. Historical CLI/session executable values are
consumed/ignored; GUI metadata v4 reads v1/v2/v3. Current binaries are under
target/embedded-git-next/release; preserve all previous paths. All Git calls remain
read-only/off render through existing versioned controllers. Keep typed status,
including distinct staged-deleted and untracked records sharing the same path.
Submodules, includes, external filters, sparse/partial/alternate-ref repos fail
explicitly instead of reporting false clean. Quotas/monitor cancellation are
cooperative, not hard process-memory/I/O deadline guarantees. Preserve these limits
and document large-index/pack peaks as pending. Writes/network/advanced Git remain M5.
Licenses and notices live in third_party/gitoxide; packaging verifies inventory
hashes and includes them. Retain attribution in files. 533 tests passed by suites,
two child helpers invoked by parents; not a global runtime workspace pass.
135 hidden own-HWND GUI checks passed with PATH empty and fixture/Git metadata
unchanged. Fmt/all-target Clippy/workspace release offline passed. No new physical
desktop/ConPTY/IME/accessibility or global performance acceptance. CI remains dormant.

## Current periodic recovery / retention closure — 2026-11-03

Read docs/PERIODIC_RECOVERY_RETENTION.md and docs/SESSION_RECOVERY.md first.
Current binaries: target/recovery-next/release; preserve all previous binary paths.
Periodic capture uses an identity/epoch-checked cursor, bounded64KiB source chunks,
queue1 and one disk worker. No Document/FileSession/undo clone or source I/O in render.
Defaults:2s idle,30s maximum admission delay,10s retry;30s is an attempt, not a
publication promise during uninterrupted edits. Completed documents retain their
initial snapshot version; later activity schedules another capture. Preserve source
identity in dedup keys to avoid ABA across workbenches. All mutation attempts that
can alter the codec invalidate the core cursor, including save/history/storage.
GC pins/validates HEAD and BACKUP and deletes only admitted obsolete generations
by Win32 handles, never recursively or through document metadata paths. Keep unknown,
hardlinked/reparse or blocked entries safe. Unpublished-generation RAII cleanup must
check publication controls before deletion. Revalidate the previous commit before
replacing BACKUP; corruption must not overwrite the only valid fallback.
Deferred GC retries without new document checkpoints. Normal exit cancels/joins
periodic work, regains the lease and confirms the final live state; failure keeps
the editor open. Disabled mode must not retain a due deadline or busy loop.
20 new tests passed by suites/directed closure plus existing recovery regressions;
three forced-exit GUI and TUI processes each passed7 native checks. Fmt/all-target
Clippy/release offline passed. No full runtime workspace pass or physical-display
acceptance claim. Tests use only owned work fixtures; personal depot untouched.
Keep CI dormant through M2–M4; do not repeat closed suites for documentation changes.

## Previous GUI shell / Rust recovery closure — 2026-11-03

Read docs/M2B_GUI_TABS_BROWSER.md, docs/M2_TUI_MOUSE.md, docs/SESSION_RECOVERY.md
and docs/CORE_GIGABYTES_ADMISSION.md before extending these routes. Current binaries
are target/gui-next/release; preserve prior target/release and target/ui-es/release.
GUI client caption/menu share #2E3440; independent pixel chrome/tree/tabs and retained
frame/DIB remain read-only consumers of the shared core. TUI mouse hits are bounded,
versioned and use grapheme/TAB/CRLF-aware coordinates; browser uses the shared catalog.
Recovery is Rust-only, immutable generations with flush/sync/readback, atomic HEAD,
previous BACKUP and exclusive per-instance leases. Keep original file bytes untouched
until explicit Save. Startup restores off-side; exit failure must keep the editor open.
Preserve original encoding provenance, disk fingerprint, undo/redo and all persistent
views. Core preflight and restore limits must agree before any generation publication.
Recovery currently checkpoints normal exit only; periodic checkpoints and safe GC are
pending. Default Windows depots live under LOCALAPPDATA/CIDE/recuperacion/{gui,tui}.
Never use the personal depot in tests: inject EditorRecovery::open_in fixtures, smoke
mode, or CIDE_STATE_DIR plus explicit CIDE_NO_SESSION=0. ConPTY defaults to disabled.
No full-editor GB claim: only isolated 5GiB sparse paged-source evidence. User chose
private snapshots; protected-source prototype is not the selected frontend policy.
All-target check, fmt, Clippy and release offline passed. Directed recovery tests,
process-exit fault test, three-process TUI and GUI restarts, 15 shell/13 interaction HWND checks
passed. Existing browser5/mouse7 ConPTY evidence remains valid; no global runtime pass.
Do not rerun closed suites for docs-only edits. CI remains dormant through M2–M4.

## Current GUI / Spanish presentation closure — 2026-11-03

Read docs/M2B_GUI_WORKFLOWS_ES.md before extending these workflows. GUI open
comparison, optional source services and snippet review are connected using F1/F3.
Own UI strings are Spanish in both frontends; keep original source, technical IDs
and external diagnostics unchanged. TUI theme follows OmniCore, GUI follows Sublime.
Shared bounded Markdown preview is inert/read-only, not full CommonMark.
549 distinct test names passed by suites/directed closure,19 new; two child helpers
run through parents. Four ConPTY flows and15 HWND checks passed. Fmt/Clippy/release
offline passed; physical/global acceptance remains open. Current executables live
under target/ui-es/release to preserve the user's previous target/release process.
Historical statements below do not override this closure. Do not repeat all passing
suites for documentation-only edits; keep CI dormant through M2–M4.


## Current Unicode/TUI closure — 2026-11-03

Read docs/M2_UNICODE_LAYOUT.md and docs/M2_TUI_FINISH.md before extending editing.
The authorized unicode-segmentation=1.13.3 and unicode-width=0.2.2 are pinned;
Rust metadata minimum1.85, existing toolchain1.85.1. Do not ask permission again.
Keyboard navigation/deletion uses grapheme boundaries; scalar/byte APIs remain
explicit. Both frontends consume VisualAtom/Glyph with narrow per-grapheme width,
TAB4, atomic clipping and strict encoding. No whole-line rectangular layout in
steady-state rendering; admission/edits retain bounded LineLayout validation.
Cache partition: scalar512KiB + Unicode256KiB + invisibles128KiB + syntax128KiB.
Source glyph payload64KiB/row,2MiB across panes;131072 screen cells;16MiB cursor
read work. Preserve visible quota errors and raw source bytes, including invalid UTF8.
Clipboard TUI owner is lazy on a dedicated event-driven Win32 thread with one
bounded request and no model access; GUI supplies its existing HWND directly.
No personal clipboard access in tests. The local scope's functional TUI blocks
are implemented; physical/global acceptance and real M3–M5 providers remain open.
530 distinct test names observed passing by suites/directed closure, two helper
children invoked by parents,18 new cases; no single full runtime workspace pass.
Historical next-block statements below do not override this current closure.


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
64KiB is per-call work,128KiB is logical syntax cache/document; text layout512KiB
plus Unicode256KiB and invisibles128KiB preserves the1MiB total cache budget. Honor NO_COLOR.

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

## Clipboard Windows TUI — cierre local 2026-11-03

Adaptador CF_UNICODETEXT de la GUI extraído a jc-platform (sin dependencias
externas); ambos frontends comparten implementación. TUI conecta transporte en
el loop real y crea HWND propio invisible bajo demanda, solo por Copy/Cut/Paste.
Headless conserva clipboard interno; pruebas inyectan memoria, no el clipboard
personal. Cortar después de publicar, pegar por destino/revisión y un undo;
cuotas/encoding estricto, campos del overlay superior y rectángulos internos.

32 casos Rust distintos aprobaron: cinco de plataforma (cuatro migrados y
uno nuevo de HWND propio), siete TUI nuevos y regresiones de clipboard/F1/registro.
Fmt, Clippy workspace/all-targets y release bins/examples aprobaron offline.
GUI HWND propio:13 checks selector-input, GDI sin error, fixture y metadata Git
intactos, clipboard sintético. ConPTY TUI: tres flujos,27436 bytes VT/salida0,
fixture intacto. No lectura/escritura de clipboard personal ni certificación OS.
Un fixture usó un nombre de método inexistente y un campo tenía visibilidad más
amplia que su trait; se corrigieron antes del cierre, logs retenidos.

`docs/M2_TUI_CLIPBOARD.md`. Sigue layout Unicode general: Juan
autorizó unicode-segmentation=1.13.3 y unicode-width=0.2.2; `docs/M2_UNICODE_LAYOUT_ADMISSION.md`.
Esa autorización sustituye la restricción previa para estas dos dependencias;
no pedirla de nuevo. Sin instalación de programas, CI ni AVM.
