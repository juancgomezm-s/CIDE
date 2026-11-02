# Continuación local — después del bloque de evidencia M2

F1/F2/F3 están implementados con evidencia local en `m2a/dual-pane-tui`. F1:
`ddc9d41`; F2: `2423134` + normalización `e72856f`; consultar el log para F3.
Guardado Windows: `ed51143`; S1 `6e07b1e`; S0 Unicode cooperativo implementado después (consultar log). No sustituir el árbol por main ni repetir checkpoints.

## Inicio

Juan autorizó continuidad operativa: no esperar otra orden de «continúa» después
de un commit intermedio. Integrar/validar cada bloque y activar enseguida el
siguiente pendiente independiente del plan. Antes de cerrar turno debe quedar un
bloque realmente activo o un bloqueo concreto que necesite decisión de Juan.
Reutilizar hasta tres agentes nativos total con archivos exclusivos; un único
integrador conserva APIs/exportaciones/manifests/comandos/plan/VALIDACION/commits.
No ampliar alcance y mantener ventanas seriales de compilación/fixtures/medición.

1. Revisar git status/rama/log y conservar modificaciones del usuario.
2. Leer AGENTS, README, PLAN, ARQUITECTURA, DECISIONES, los checkpoints F1/F2/F3,
   S0_DOCUMENT_SEARCH, S1_PROJECT_SEARCH, SAVE_WINDOWS_CONTENTION y VALIDACION, bajo `docs/`. Mediciones crudas en `docs/benchmarks/`.
3. Revisar `jc-app/src/jobs.rs`, `project_search.rs`, `project_search/scan.rs`,
   `project_search/replace.rs`, `workbench/text.rs`, `panes.rs`,
   `jc-tui/src/app/presentation.rs`, `render/mod.rs`, `frame_encoder.rs`,
   `event_loop.rs` y `terminal.rs` dentro de sus crates.
4. Validar localmente mediante `scripts/cargo-local.ps1`; no ejecutar Actions
   (M5). Tests de archivo requieren hard links/ReplaceFileW y usan temporales
   del proyecto. No descargar dependencias nuevas sin necesidad concreta.

## Contratos existentes

Un documento/sesión/undo y estado persistente único por vista. Transacciones
atómicas por revisión con mapa completo. Core byte-preserving, lectura indexada
por segmentos/rangos, proyección por revisión/encoding, EOL y checkpoints
incrementales. Layouts 1 MiB/documento, historial lógico 8 MiB/1024 pasos y
recolección conservando los undos retenidos. Cerrados conservan descriptores.

Preparación mutable antes de `Presentation` prestada; composición de solo lectura
y encoder de diferencias con baseline válido. No reintroducir reveal ni consultas
textuales que muten cachés dentro del renderer. Entrada y resultados despiertan la
UI con Wakeup; drenar lotes acotados y comprobar backlog antes de dormir.

JobSystem usa snapshots propios del rango, revisión/encoding/vista y generaciones.
Cuotas cubren pendientes, ejecución y completados sin entregar. Cancelación
cooperativa, no preempción de proveedores que ignoren el token. Workers no reciben
Workbench ni escriben buffers/disco. Propuestas requieren futura aceptación y
`apply_text_edits` por revisión; no aplicar desde poll_tasks. Configuración distinta
de encoding deberá cancelar/reemplazar explícitamente la clave. El sistema actual
está ligado a un Workbench y documentos, no a trabajos de proyecto sin documento.

## Siguiente bloque: picos/sufijos restantes y medición conjunta de Git

El bloque Git/tareas/diff/TUI está guardado localmente en `6cf6f13`. El bloque
posterior ya midió sufijos y auxiliares, y acotó el crecimiento de replace_after:
leer M2_SUFFIX_COST_REVIEW y GIT_AUXILIARY_MEASUREMENT. Suite completa 240 pruebas,
fmt/Clippy/release; 4914 observaciones antes y después, y nueve sesiones Git.
No volver a ejecutar lo aprobado sin cambios o preocupación material.

Los frentes cold_growth y dense-edits ya fueron recogidos, revisados y ejecutados
por el integrador. M2_COLD_GROWTH_DISTRIBUTION conserva tres pares con igual
ejemplo/versiones identificadas: 5580 filas, sin cambio consistente de signo en
p95 frío. M2_DENSE_EDIT_EVIDENCE registra catorce procesos retenidos y sus checks:
pico histórico de compromiso 68,160→52,461 MiB; la puerta de 50 MiB sigue abierta.
No repetir esos datasets ni reemplazarlos por otras corridas sin motivo material.

Bloque siguiente realmente en construcción: `examples/support/block_line_index.rs`
y `examples/line_index_candidates.rs`, dentro de jc-app, con método
LINE_INDEX_CANDIDATES y contrato LINE_INDEX_CANDIDATE_CONTRACT. Candidato y driver
se construyen en archivos exclusivos; no hay cambio productivo ni decisión de
estructura. El integrador debe esperar/recoger ambos, revisar, abrir ventana
serial y medir antes de seleccionar. No cerrar turno afirmando que siguen activos
si ya terminaron sin un integrador que recoja sus entregas. Distinguir siempre
modelo, proceso nativo e imagen física; no tocar configuración del sistema.

- Construir bloques más amplios antes de validar, según pidió el usuario. Reservar
  comprobaciones intermedias para bloqueantes concretos; no repetir suites por retoques.
- S0 está implementado: rangos cooperativos, Unicode 17 full folding, palabra,
  selección, historial, marcadores y Go to. Un solo matcher sirve documento/proyecto;
  no restaurar el escaneo síncrono, ASCII case-insensitive ni KMP paralelo.
- Decoder/scratch ya optimizados: `S0_DECODER_OPTIMIZATION.md`. Un iterador
  prestado sirve también a decode/decode_fragment; buffer reutilizado y rechazo
  ASCII sin prefijo dentro del mismo matcher. Proyecto p95 18,649 ms frente a
  40,930 ms contemporáneos y 27,457 ms S1 histórico en el mismo corpus. No extrapolar
  esa mejora a todos los casos ni afirmar mejora de cancelación. Conservar cuotas.
- Corpus Unicode/legacy, 4097 piezas, ocho documentos, teclado y memoria por
  proceso ampliados: `M2_ACCEPTANCE_EVIDENCE.md`. Layout frío ya recorre 64 KiB
  con contexto, sin String/offsets de toda la línea. Pico real de línea ASCII
  16 MiB: 434,9 → 18,0 MiB. No reintroducir esa materialización.
- Metadatos densos ya compactados: `M2_DENSE_LINE_METADATA.md`. Cuatro bytes
  por línea para offsets de 30 bits y promoción sin pérdida; no restaurar dos
  vectores espejo. Recolección reutiliza arenas completamente alcanzables.
  Modelo tras frío ~49,3 MiB privados; pico nativo 71,9 → 51,7 MiB, todavía
  sobre 50 MiB. Preservar presupuesto/cache y atender picos/sufijos restantes.
- Matriz de teclado ampliada, 209 pruebas integradas y gate central del dispatcher
  TUI para impedir ejecución de menús deshabilitados. Ampliar matriz física/manual
  y latencia de pantalla; regex continúa sin motor real.
- Mantener preview/confirmación/huellas de S1 y transacciones. Abiertos no se guardan
  automáticamente; cerrados usan FileSession. Workers no escriben.
- Guardado Windows conserva retries 32/33/1175 y recovery 1176/1177; no borrar el
  temporal de recuperación ni cambiar por delete+rename. Causa histórica no atribuida.
- Prototipo GUI según PLAN, compartiendo stores/comandos; medir backends antes de
  elegir. Sublime Text es referencia visual y ecode referencia operativa. Progress M3.

## Medición y límites

El bloque de evidencia pasó 194 pruebas completas Windows, fmt, Clippy/release,
24 sesiones ConPTY antes y 24 después, y regresión Unicode nativa
(ver VALIDACION y M2_ACCEPTANCE_EVIDENCE). La integración paralela posterior pasó
200 pruebas en aquel checkpoint: DiffModel puro y estudio GUI estático sin backend
(DIFF_MODEL_READ_ONLY, M2B_SCENE_STUDY). Revisión renderizada del HTML pendiente;
Edge headless terminó con 0xC0000005, sin modificar el sistema. Benchmarks opt-in:
`document_search_bench`, `project_search_bench`, `f3_pipeline`, `f3_jobs`; smoke Windows `scripts/smoke-conpty.ps1 -ProjectSearch`, tras compilar
`f3_console_host` y `jc-ide-tui` en release. Método en S0_DOCUMENT_SEARCH, S1_PROJECT_SEARCH y F3_PRESENTATION_TASKS.

ConPTY registra inyección-a-VT (p95 16,0726 ms en fixture mínimo), no pantalla
física. Un muestreo corto de memoria/CPU de ese proceso no certifica metas globales.
Ya hay arranque/memoria por proceso y doble grupo nativo medidos en M2_ACCEPTANCE_EVIDENCE;
mantener abiertas aceptación física/global, caché fría real, servicios y cargas densas. El código 1175 ya tiene reproducción nativa observada e inyección determinista;
el tratamiento seguro se probó junto con bloqueos persistentes y recovery. No
confundirlo con eliminación de toda contención externa.
F2 ya midió rangos/modelo hasta 16 MiB; no confundir
esas llamadas con latencia UI. Revisar líneas extremas, metadatos O(L), muchas
piezas y ancho Unicode/grafemas/IME/DPI. M2A continúa abierto.

Conservar bytes, BOM/EOL/CRLF, encoding explícito, guardado seguro, revisiones y
vistas independientes. No tocar Qwen, GPU, drivers, startup ni servicios ajenos.
No hacer push ni operaciones remotas sin autorización. Git con escritura y CI en
M5; plugins externos post-M5. Entregar evidencia y commit local sin reescribir
historial.

S0 optimizado medido en 16 MiB: programar p95 3 µs, slot p95 180 µs y fin p95 178829 µs;
son tiempos de modelo, no pantalla física. PowerShell tuvo bloqueo de inicialización
en esta sesión: se validó con Cargo directo en el mismo entorno de proceso y el host
C# existente compilado con csc. No se modificó configuración del sistema. El wrapper
normal sigue disponible; no investigar/tocar servicios ajenos para validar el editor.


Proveedor Git posterior: GIT_READ_ONLY_PROVIDER.md y git_read.rs ya leen blobs
HEAD/index exactos con identidad/OID, límites, deadline/cancelación y Job Object
Windows. Suite integrada 218 pruebas (dos helpers son fixtures invocados por los
tests), fmt/Clippy/release; Git 2.54.0.windows.1. La integración posterior ya conecta menú/palette, renderer de solo lectura y
JobSystem/TaskStamp: GIT_DIFF_DELIVERY y GIT_DIFF_PRESENTATION. Cierre con 237
pruebas aprobadas, fmt/Clippy/release y ConPTY HEAD/index sin otra tecla, navegación,
undo→Stale y conservación de disco/index. El siguiente incremento mide costes
conjuntos de editor + auxiliares y atiende picos/sufijos restantes;
no llamar read_blob desde entrada/render ni habilitar comandos sin handler real.
No eliminar reap/cancelación de readers para fingir un retorno de cinco segundos.
El callback debe ser barato; kernel cleanup no es preempción dura.
