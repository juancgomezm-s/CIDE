# Contrato de construcción — Git diff versionado

Contrato mínimo fijado antes del reparto, 2026-11-02. El integrador conserva jobs,
OwnedDiffModel/APIs compartidas, exportaciones, comandos, frontend glue, plan,
validación y commits. No se incorpora otro pool: se amplía JobSystem F3.

## Worker aplicación: git_diff.rs

Propiedad exclusiva: `crates/jc-app/src/git_diff.rs`,
`crates/jc-app/tests/git_diff.rs`, `docs/GIT_DIFF_DELIVERY.md`.

- `GitDiffOptions { git_executable: PathBuf, root: PathBuf, cwd: PathBuf,
  source: GitBlobSource }`, Clone/PartialEq/Eq.
- `GitDiffState`: Idle, Queued, Running, Ready, Limited, Cancelled, Stale,
  Failed, TimedOut.
- `GitDiffController::new()`, request(app, jobs, options)->Result<(),String>,
  tick(app,jobs)->bool, accept(app,&mut CompletedTask)->bool,
  cancel(jobs), clear(jobs), state(), message()->&str, result()->Option<&GitDiffResult>,
  selected()->usize, select_relative(isize), select_hunk(index,&mut Workbench)->Result<(),String>.
- Constante pública `GIT_DIFF_JOB_KEY` exclusiva del controlador.
- `GitDiffResult { identity: GitBlobIdentity, model: OwnedDiffModel }`.
- `GitDiffReply { generation:u64, stamp:TaskStamp, result:Result<GitDiffResult,String> }`
  con `memory_bytes()->usize` que cobra capacidad de fuentes/modelo/identidad/error.
- `pub(crate) GitDiffWork { generation:u64, stamp:TaskStamp,
  options:GitDiffOptions, document_path:PathBuf }` y
  `pub(crate) run(work, &TaskSnapshot, &Cancellation, &mut TaskOutput)->Result<(),TaskFailure>`.

APIs que entrega el integrador: `JobSystem::submit_git_diff(app,key,work)` (crate),
TaskSnapshot::shared_bytes()->Arc<Vec<u8>> (crate),
TaskOutput::set_git_diff(reply)->Result<(),TaskFailure> y take_git_diff()->Option<GitDiffReply>.
`GitBlobSnapshot::into_parts()->(GitBlobIdentity,Vec<u8>)` mueve la fuente.
`diff_model::OwnedDiffModel::compute(Arc<Vec<u8>>,Arc<Vec<u8>>,DiffLimits,cancelled)`
usa el mismo motor y ofrece status/stats/spans/hunks/line_count/line_byte_range/
line_bytes/source_bytes/corresponding_line/memory_bytes, como el modelo prestado.

Snapshot de documento entero máximo 2 MiB; blob Git 2 MiB; metadatos diff 4 MiB;
artefacto reservado 12 MiB, uno Git en vuelo y un pendiente solo de metadatos.
JobSystem cobra reserva desde admisión hasta poll; timeout Git-diff 10 s, Git
reader conserva 5 s. No mantener pool/queue propios. Request captura intención;
la copia acotada la hace JobSystem. Git/path canonicalization/diff solo en worker.
Superseder cancela en vuelo y sustituye pendiente sin otra copia de bytes. Poll/
tick no hacen I/O y descartan revisión/encoding/vista/documento/ruta obsoletos.
HEAD/index se muestran como snapshots de OID, no como watcher del estado actual.
select_hunk solo navega el documento de trabajo si sigue vigente; no edita.

## Worker presentación: render/git_diff.rs

Propiedad exclusiva: `crates/jc-tui/src/render/git_diff.rs` y
`docs/GIT_DIFF_PRESENTATION.md`. Consume únicamente presentación preparada.
Funciones `pub(super) sidebar(screen,&GitPresentation,x,y,width,height)` y
`pub(super) unified(...)`, sin consultar Workbench/servicios ni modificar selección.

Tipos que entrega el integrador, reexportados desde crate::app:
`GitPresentation { status:String, details:Vec<String>, hunks:Vec<GitHunkRow>,
rows:Vec<GitDiffRow>, total_hunks:usize }` (Default).
`GitHunkRow { index:usize,label:String,selected:bool }`.
`GitDiffRow { kind:GitRowKind,left_line:Option<usize>,right_line:Option<usize>,
text:String,eol:&'static str }`; números presentados one-based.
`GitRowKind { Context, Deleted, Added, Header, Notice }`.
Screen usa solo Normal/Reverse/Dim; signos y EOL deben ser explícitos y todo se
recorta al área. Los textos recibidos ya son prefijos visibles acotados. Tests
locales del componente comprueban geometría, selección y solo lectura.

## Integración / ventanas

El integrador habilita acciones reales HEAD/index/refresh/cancel y navegación,
vista Git del archivo activo más panel unificado virtualizado. No status de todo
el repositorio ni escrituras Git. Git se configura por ruta absoluta explícita;
el proyecto abierto debe ser la raíz. Trabajadores solo diseño/código; Cargo,
fixtures Git y benchmarks esperan la ventana del integrador. No se repite GUI.
