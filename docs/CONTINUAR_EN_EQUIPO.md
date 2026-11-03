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

## Checkpoint actual: índice por bloques integrado

Git/tareas/diff/TUI está en `6cf6f13`; crecimiento plano y medición inicial en
`d6de969`; distribuciones en `6feb19b`; candidato/comparador en `191bd82`.
Ahora `workbench/line_index.rs` contiene un único índice por bloques, con bases
usize y registros relativos/EOL u16. El plano y los ejemplos que dependían de él
fueron retirados, conservados en la historia. Leer M2_BLOCK_LINE_INDEX y
M2_UNDO_PIPELINE_PROBE antes de tocar esas rutas. No reactivar dos modelos.

Bloque completo construido antes de validar: migración, oráculos multibloque,
aplicación real UTF-8/legacy/vistas/undo, cuotas y liberación de catálogo. Suite
completa 245 aprobadas; ajuste final de catálogo con 15+3 pruebas afectadas,
246 distintas al cierre. fmt/Clippy/release aprobados. No repetir lo aprobado
sin cambio o preocupación material. Ejemplo pipeline también compilado/ejecutado.

Dos pares Workbench, 48 sesiones nativas / 42 retenidas y dos pares in-process
están recogidos, revisados y medidos serialmente, no pendientes en agentes.
Pico denso editado 43,73/43,82 MiB en dos capturas; local bajo 50 MiB, sin aceptación
global. Undo interno total p95 179,2/172,1 µs frente a plano 573,7/752,9 µs; VT
repetido 16,9147 ms bloques / 16,9702 ms plano. La cola nativa inicial mayor no
persistió, se conserva. No fabricar latencia residual restando percentiles.

Pendientes funcionales del plan: regex real acotado, selección rectangular con
clipboard/undo, símbolos/includes/RUN mediante proveedor, y backend GUI medido.
No habilitar switches decorativos ni mezclar proveedores ABL en jc-app. No más
intentos GUI/browser sin una ventana autorizada; no cambios de sistema/servicios.
Puerta física requiere medición de presentación real, separada de VT/modelo.

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
- Metadatos por bloques ya integrados: `M2_BLOCK_LINE_INDEX.md`; la representación
  plana de `M2_DENSE_LINE_METADATA.md` es histórica. Bases nativas y relativos/EOL
  sin truncamiento, una única ruta. Recolección sigue reutilizando arenas
  completamente alcanzables. Preservar presupuestos, heap accounting y tradeoffs.
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


## Prioridad vigente: CPU y concurrencia acotada — 2026-11-02

Juan pide aprovechar núcleos disponibles. El bloque Git proveedor/controlador
está cerrado con 261 pruebas completas, fmt/Clippy/release; su presentación TUI
se pospone ante esta prioridad. Leer GIT_STATUS_READ_ONLY y GIT_STATUS_DELIVERY.

Primero inventariar hardware real y perfilar búsqueda por archivos, diff,
decodificación y tareas de fondo. Elegir el cuello medido; reutilizar planificador
cuando encaje, concurrencia configurable/acotada, cancelación, cuotas/backpressure,
resultados deterministas/versionados, sin pools anidados ni copias grandes.
Preservar bytes/encoding/undo y no paralelizar escrituras de un documento.

Antes/después con corpus/protocolo idénticos: throughput, latencia/p95 del tramo
medible, CPU por proceso/tiempo y memoria/picos. 100% CPU no es aceptación.
Reservar margen para Qwen; sin afinidades/prioridades/servicios/GPU ajenos ni
archivos privados. Si no puede verificarse su actividad sin interferir, registrar
carga concurrente desconocida y no atribuir toda variación al cambio. Máximo
tres agentes, un integrador, compilaciones y mediciones pesadas serializadas.
Construir bloques completos antes de validar. Sin instalaciones, push ni CI.


## Cierre CPU y siguiente bloque — 2026-11-02

CPU implementado y medido: [CPU_PERFORMANCE](CPU_PERFORMANCE.md). Search Plus usa
un solo engine por archivo, default dos trabajadores incluido coordinador,
configurable 1…4 con admisión/headroom conservadores. TUI ya acepta
`--search-workers 1..4`; uno reduce demanda mientras se trabaja con otras cargas.
Se conservan bytes/encoding/undo, revisiones, cancelación y cuotas globales; helpers
lazy reutilizados sin snapshots duplicados ni pools anidados/F3 documental ficticio.

Cuatro capturas / 1488 consultas conservadas, mismo probe/corpus y dos órdenes.
16 MiB: mediana 1,24–1,49× más rápida, uso agregado ~1→1,73–1,80 núcleos equivalentes;
CPU total aumenta 30–48%. Memoria privada muestreada del harness baja de
5,527–5,625 a 3,520–4,230 MiB, incluido reader sin crecimiento para guardia EOF.
Un caso pequeño tuvo mediana peor; no es aceleración universal ni menos CPU total.
Codegen jc-app s/2/3 se comparó aparte: se **mantiene s**, sin mejora UTF-8 consistente
que justifique cambiar política global. Qwen/carga, energía y temperatura no
observados/controlados; no tocar procesos ajenos. Pantalla física/global RAM abiertas.

Validación CPU: 269 pruebas distintas al completar las suites tras corregir dos
oráculos S1 sensibles a mayúsculas; luego integración de arranque TUI, 47 pruebas
TUI/CLI aprobadas con cinco nuevas, total **274 distintas**. Formato, Clippy
workspace/all-targets del executor y dirigidos posteriores, release y CLI real
pasaron. No una suite completa de 274 en una sola corrida. No repetir perfiles
ni suites por documentación; sin instalaciones/CI/push/afinidades/prioridades.

**Siguiente pendiente autorizado:** presentación TUI del estado Git de repositorio,
sobre el proveedor y controlador ya implementados en `16071e3`; leer
GIT_STATUS_READ_ONLY/GIT_STATUS_DELIVERY antes de conectar. Preservar paths byte
opacos, rechazo de filtros, configuración aislada, generación/opciones vigentes,
lectura visible y ausencia de escrituras. No prolongar optimizaciones CPU sin
otra pregunta concreta ni activar un incremento ajeno al plan.

## Cierre del siguiente incremento: Git status TUI — 2026-11-02

La presentación TUI pendiente en el cierre CPU ya está conectada al controlador
existente: consulta explícita de repositorio sin documento, rutas byte opacas,
filas visibles acotadas, selección, refresh/cancel y configuración con intención.
Entrega vigente solo por poll_tasks, composición de solo lectura y recuperación
tras pánico incluso obsoleto. No watcher ni rutas editables o escrituras Git.

Bloque integrado antes de validar: 290 pruebas completas aprobadas, fmt,
Clippy workspace all-targets y release bins/examples. [Contrato/uso y límites](GIT_STATUS_PRESENTATION.md)
y [evidencia](VALIDACION.md). CPU queda cerrado con sus datos existentes; no se
repiten perfiles ni pruebas por ajustes documentales. Las menciones anteriores
de presentación status pendiente describen checkpoints históricos. Navegación
segura desde paths Git, otras funciones M2 y aceptación global de RAM/pantalla
siguen pendientes; no se declara cierre de M2A ni se activa otro incremento aquí.

## Rectángulo de una acción y siguiente bloque activo — 2026-11-02

Implementado el [rectángulo acotado](M2_RECTANGULAR_SELECTION.md): selección real
por celdas, TAB4/virtual, clipboard interno rectangular, copy/cut/paste/entrada/
Delete/Backspace con transacción única, comandos y Palette, paint readonly.
No es cierre del modo columna Unicode general: rechaza grafemas/anchos no
admitidos; las ediciones efectivas terminan el rectángulo. No OS clipboard ni GUI.

Cierre local: 310 pruebas distintas aprobadas en cierre selectivo después de
corregir un acceso que exponía también selección lineal; fmt/Clippy/release.
No repetir pruebas aprobadas ni perfiles por documentación. El siguiente pendiente
M2A está activo: revisión/contrato de unir y dividir líneas genéricos del anexo
ABL_EDITOR_COMMAND_SCOPE. Join usará rangos de separadores/indentación y un batch
F1; Split reutilizará insert_newline/preferred EOL. No ampliar a AVM/GUI, Unicode
general ni otra optimización CPU sin una necesidad concreta.

## Join/Split cerrados y siguiente admisión activa — 2026-11-02

Completado [unir/dividir líneas](M2_LINE_COMMANDS.md): modelo universal,
transacción única, lectura de prefijos/EOL por rangos y clasificación byte→línea
sin layout escalar. Split reutiliza la inserción de newline existente; Enter se
centraliza y Ctrl+Shift+J no interfiere con Ctrl+J. Preservados BOM/EOL final,
encoding/bytes fuera de cortes, vistas y undo. No normalización sintáctica.

Validación: 74 dirigidas de app y cobertura TUI completada tras corregir solo un
fixture de foco; nueve nuevas, 319 distintas al cierre, fmt/Clippy/release.
No una suite única319 ni una repetición de perfiles. Contrato TUI se reconcilia
con las funciones implementadas y límites actuales, sin inferir recorrido físico.

**Siguiente bloque activo:** auditoría/admisión de regex real acotado de Search
Plus, prioridad universal pendiente de M2A. Revisar engine compartido y contrato
de bytes/Unicode, presupuesto/cancelación, capturas/reemplazo y costo sin instalar
dependencias. No habilitar el switch hasta que haya un motor operativo y validado;
no adelantar proveedores de extensión/AVM M3 ni GUI sin ventana autorizada.

## Admisión regex completada: decisión de dependencia pendiente

La auditoría siguiente ya está recogida en [M2_REGEX_ADMISSION](M2_REGEX_ADMISSION.md).
No se habilitó regex ni se descargó nada. Candidato para evaluar: regex-automata
0.4.18 mantenido, con compilación/VM cooperativa, presupuestos, coordenadas y
reemplazos explícitos; no asumir que una llamada monolítica es cancelable.
Lock/cache no contienen un motor. La instrucción de continuidad «sin instalar
dependencias» exige decisión de Juan antes de añadir/fijar esa dependencia Rust.
Alternativa: conservar regex deshabilitado y seleccionar otro pendiente M2A.
No implementar un parser/engine casero para esquivar la restricción. La admisión
es diseño/revisión de fuentes primarias y código local, sin pruebas/benchmarks
nuevos ni aceptación de un motor; siguen vigentes los 319 casos del cierre previo.

## Prioridad vigente de Juan: construir M2A y M2B — 2026-11-02

Juan cerró el plan correctivo y pidió continuar el roadmap original: «Continúa
con la construcción, síguete con M2A y M2B»; aclaró que automatizamos hasta donde
sea posible y solo deja las pruebas de usuario para el final. Las puertas globales
RAM/pantalla quedan documentadas, sin bloquear construir funciones ni abrir
optimizaciones indefinidas. Regex permanece pendiente sin autorización para
agregar/descargar/fijar regex-automata u otra dependencia; no volver a pedirlo para
avanzar un pendiente independiente.

Frentes admitidos en el árbol actual: [Quick Open M2A](M2_QUICK_OPEN.md), reservado
Ctrl+P del roadmap, y [primer GUI Windows M2B](M2B_NATIVE_WINDOWS.md) mediante
Win32/GDI nativo y Workbench compartido. Dependencias del GUI: solo paquetes
existentes jc-app/jc-text, sin descarga de framework. El HTML de prototypes sigue
siendo estudio histórico, no el ejecutable. Un integrador, hasta tres agentes
totales, archivos exclusivos y validación pesada serial al cerrar bloques amplios.
No CI/push/instalaciones, cambios de seguridad/servicios ni Qwen/GPU.

Estado al activar: implementaciones en construcción/integración, sin anunciar aún
pruebas de este bloque. El GUI real tendrá smoke de HWND propio oculto y renderer
GDI; no controla otras aplicaciones. Validar compilación, comportamiento compartido,
reporte/bitmap y límites, y dejar checklist manual para Juan sin pedir prueba ahora.

## Primer cierre funcional M2A+M2B y siguiente bloque activo

Quick Open de proyecto TUI y GUI Win32/GDI inicial implementados: consulta
cancelable/acotada, revalidación/apertura confinada y sesión única; ventana real
con edición/selección/undo/grupos/guardar y frame readonly. [Uso/límites finder](M2_QUICK_OPEN.md)
y [GUI/evidencia nativa](M2B_NATIVE_WINDOWS.md).339 pruebas Rust distintas en
cierre selectivo +16 checks del ejecutable real, fmt/Clippy/release aprobados.
No cambios de dependencias externas, CI/push o perfiles repetidos.

M2B continúa en construcción: no declarar paridad completa ni puertas físicas
cerradas por ese smoke. El siguiente bloque ya admitido/construyéndose es adaptar
Quick Open y Command Palette al GUI, con selección/input acotados y presentación
GDI readonly, usando los controllers y comandos existentes. Una notificación al
loop nativo debe recibir completions sin otra tecla y sin polling permanente.
Regex permanece pendiente; no solicitar su dependencia para avanzar este frente.
Las pruebas manuales de usuario se conservan para el final.
