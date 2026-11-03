# Continuación local — después del bloque de evidencia M2

## Estado vigente — GUI y TUI en español, 2026-11-03

Comparación de documentos abiertos, servicios de fuente opcionales y plantillas
están conectados en GUI. Ambas interfaces incorporan español y Markdown de
lectura; TUI añade tema inspirado en OmniCore, letras Alt subrayadas y ayuda lateral.
[Contrato, uso y evidencia](M2B_GUI_WORKFLOWS_ES.md).
549 nombres de pruebas aprobados por suites/cierres dirigidos,19 nuevos; dos
helpers se ejecutan por sus padres. Fmt/Clippy/release offline, cuatro recorridos
ConPTY y15 checks HWND propios aprobados. No corrida runtime global única.

Ejecutables actuales: `target/ui-es/release/cide-tui.exe` y `cide-gui.exe` en esa
misma carpeta; el antiguo `target/release` se conserva. F10/Alt abre menús, F6
cambia panel y Ctrl+Alt+M alterna Markdown. Se preservan F1/F2/F3, bytes y undo.
Sin nuevas dependencias externas; GUI usa el proveedor opcional existente.

Quedan abiertas aceptación física/global de RAM/arranque/pantalla, IME/DPI y
accesibilidad/clipboard real. AVM M3, semántica M4, Git escritura/CI M5 y runtime
externo post-M5 conservan su alcance. Sin CI, push ni publicación.
Los siguientes estados y pendientes históricos se subordinan a este cierre.

## Estado vigente — cierre funcional TUI, 2026-11-03

Los bloques admitidos de TUI están implementados y validados localmente: regex,
comparación de abiertos, símbolos/includes/RUN léxicos opcionales, completado
textual, snippets, foco/overlays, clipboard Windows explícito y layout Unicode.
[Matriz vigente](M2_TUI_FINISH.md), [contrato Unicode](M2_UNICODE_LAYOUT.md).
530 nombres de pruebas aprobadas por suites/cierres dirigidos,18 casos nuevos;
dos helpers Git se ejecutan por sus padres, no una corrida global única.
Fmt/Clippy/release offline, cuatro flujos ConPTY y17 checks HWND propios aprobados.

Se preservan bytes/encoding/BOM/EOL/undo, F1/F2/F3 y un único modelo editable.
Unicode: grafemas/narrow/TAB4, caché512+256+128+128KiB/documento, filas64KiB,
presentación fuente compartida2MiB, pantalla131072 celdas. Clipboard TUI tiene
hilo Win32 lazy con buzón único y propietario que responde inactivo; pruebas sin
abrir clipboard personal. Las dos bibliotecas autorizadas están fijadas y el
mínimo declarado es Rust1.85; toolchain existente1.85.1, sin instalar programas.

La siguiente puerta es aceptación física/global con terminal/fuente del usuario,
clipboard real y mediciones RAM/arranque/pantalla; no se cierra con modelo/ConPTY.
No ampliar automáticamente a AVM/semántica M3–M4, Git escritura/CI M5, plugins
externos post-M5 ni gestos de ratón sin admisión. El paquete de desarrollo y sus
hashes están en los metadatos Unicode. Sin CI, push o publicación.

Las secciones fechadas siguientes conservan decisiones/evidencia históricas;
sus antiguos «siguientes» y «pendientes» se subordinan a este estado vigente.


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


## Último cierre: selectores GUI nativos; siguiente clipboard/encoding

Leer docs/M2B_NATIVE_SELECTORS.md.350 pruebas completas y23 checks reales nativos,
Clippy/release; Quick Open/Palette/carpeta ya conectados. No repetir suites por
documentación ni perfiles cerrados. Siguiente bloque amplio M2B: clipboard del
sistema y selector explícito de encoding. No activar regex/dependencias/CI ni pedir
prueba manual ahora. Preservar notifier opcional corto fuera del ticket mutex,
detach serializado con PostMessage, aceptación solo en TASKS_MESSAGE y composición
de selector visible/solo lectura. Clipboard OS todavía no certificado por smoke.


## Último cierre: las tres funciones pedidas, TUI y GUI

Leer docs/M2_INVISIBLES_MULTILINE.md y sus evidencias. Invisibles, buscar y
reemplazar bloques multilínea están integrados; clipboard lineal/encoding GUI
del bloque previo también.376 pruebas distintas por suites, Clippy/release y29
checks nativos pasaron. No repetirlos como construcción nueva ni correr perfiles
cerrados. Logs retienen los fallos de integración/fixture corregidos.

Mantener bool común de invisibles; ASCII/Range bytes por celda, CRLF atómico,
byte↔display explícito, sin marcas en copy/save/undo. No comparar scalar ni
rectángulo lógico contra columnas de escape. Cache896+128KiB/documento; diagnóstico
frío invalida por revisión. Campos compartidos4096 UTF8, EOL exactos, F2 LF/F4CRLF/
F8CR/F7TAB, CtrlU clear, F3/ShiftF3, Enter/F6 uno, CtrlEnter/F9 todos. GUI acepta
slots solo en TASKS_MESSAGE; drena input/paint antes de más trabajo y no repinta
un slot sin cambios. OSclipboard solo por acción explícita, fuera de préstamos,
con handoff que revalida doc/view/rev/encoding/caret/anchor. Smoke usa transporte
en memoria, no el clipboard del usuario. No inferir interoperabilidad OS.

Siguiente pendiente M2B: Save All con Save As/cancelación explícita; seguir
construcción amplia y una ventana serial de validación al cierre. No instalar,
activar regex, correr CI/push ni pedir prueba manual ahora.


## Último cierre: Save All GUI

Leer M2B_SAVE_ALL.md. Nuevo SaveTarget valida id/rev/path/new tras modal y requiere
destino explícito para un nuevo documento. Save All cap1024 ids únicos/metadata,
avanza solo tras éxito, detiene por cancel/error con saved/pending y no mueve
vistas/undo. Save As normal captura target antes del diálogo.45 tests dirigidos,
6 nuevos, histórico382 distintos y32 checks nativos aprobaron; no repetirlos.
Próximo bloque M2B es Search Plus de proyecto en GUI sobre el S1 existente, sin
motor ni índice nuevos. Conservar exact bytes/encoding, snapshots acotados y
entrega versionada. No CI/push/dependencias/regex, manual al final.

## Último cierre: CIDE, iconos y Search Plus GUI

Leer CIDE_REBRAND.md y M2B_PROJECT_SEARCH_NATIVE.md. Comandos canónicos cide,
cide-tui y cide-gui, aliases jc-ide compatibles; crates/raíz/remoto intactos. Icono
JG+CIDE original verde para TUI/CLI; naranja para GUI. Siete tamaños embebidos en
los seis PE, título/icono nativos comprobados. Recursos/procedencia en assets/branding.

GUI usa S1 existente: tres drafts acotados, globs, worker1..4, lotes por evento,
resultados solo lectura, Enter validado, refresh, stale/cancel/root invalidation.
open_project_file_at_version rechaza antes de cambiar catálogo/foco; no aplanar el
buffer para comparar y conservar comprobación de disco al reutilizar un abierto.
391 casos distintos aprobados por suites260+131, fmt/Clippy/release y41 nativos.
Evidencia cide-project-native-2026-11-02-* conserva fallos y hashes. No repetir esas
suites/perfiles por docs. Paquete local de desarrollo, sin instalar ni publicar.

Pendientes funcionales M2B: replace de proyecto y diff/status read-only gráficos;
manual IME/clipboard/DPI/accesibilidad y gates globales permanecen al final. No
motor nuevo, dependencias, regex, CI/push ni tocar RAG/servicios de otros proyectos.

## Último cierre: reemplazo y Git GUI — 2026-11-03

Leer M2B_PROJECT_REPLACE_GIT.md. Proyecto replace comparte S1: draft4096UTF8,
Enterpreview, F9/CtrlEnter confirmados, un archivo por TASKS_MESSAGE, Esc conserva
terminados y cancela pendientes; no-op conserva redo y cerrados no se reescriben.
FileSession.open_in_root_at_version sustituyó lectura/apertura duplicadas del commit
cerrado. No buffers/undo/planes clonados ni reemplazos desde workers/pintura.

GUI status usa GitStatusController; diff HEAD/index usa GitDiffController + F3
JobSystem lazy. Executable explícito por selector o --git-executable ABS. Solo
poll_git en TASKS_MESSAGE acepta completions. validate_context de GitDiff separa
invalidación de tick/admisión; prepare solo metadata+prefijos de readmodels.
Paths status no se abren; hunks Enter revalidados, source/encoding/EOL preservados.

53 pruebas Rust distintas dirigidas y58 nativas aprobadas, fmt/Clippy/release;
8 nuevas, histórico399 (no corrida399). Evidencia m2b-replace-git-final-2026-11-03-*.
No repetir suites ni CPU/RAM perfiles cerrados. Próximo frente activo: Explorer
lateral gráfico con catálogo común lazy/cancelable/acotado. Construir e integrar
antes de otra ventana de tests; manual al final. No CI/push/dependencias/regex ni
servicios/procesos ajenos; ningún componente .NET8 apareció en la revisión previa.

El siguiente contrato ya está preparado en M2B_EXPLORER_CONTRACT.md: hoy TUI enumera
síncronamente y nombres usan to_string_lossy; Quick Open no es un árbol. Implementar
un catálogo común con IDs y worker lazy, retirar la ruta TUI anterior al integrarlo,
y conectar sidebar GUI/teclado/mouse antes de probar. Ese contrato histórico ya
tiene el cierre de implementación siguiente.

## Último cierre: Explorer compartido — 2026-11-03

Leer M2B_SHARED_EXPLORER.md. ProjectExplorer/ExplorerView de jc-app reemplazan el
proveedor/aperturas anteriores de TUI y alimentan sidebar GUI. No restaurar read_dir
en el owner, labels lossy, copias de documentos ni aceptación durante prepare/draw.
Conservar IDs/generaciones, rechazo de cancelados, cuotas globales y apertura
revalidada aun para dirty reuse. Windows mantiene cadena ordinaria sin share-delete
y enumera por handle; no presentar fallback no Windows como confinamiento atómico.

352 pruebas relacionadas aprobaron,13 nuevas/una obsoleta retirada;70 checks de
HWND propio y seis flujos ConPTY propios, fmt/Clippy/release aprobados. Evidencia
m2b-explorer-2026-11-03-* conserva fallos iniciales de los arneses y fuentes/binarios.
No repetir suites/perfiles aprobados para documentación. No solicitar pruebas
manuales ahora: clipboard OS/IME/DPI/accesibilidad/RAM/pantalla siguen al final.
Sigue M2B: paridad GUI pendiente y API de proveedores opcionales conforme al plan;
no afirmar compilación OpenEdge, parser/LSP ni loader de extensiones inexistentes.

## Último cierre: navegación nativa — 2026-11-03

Leer M2B_NATIVE_NAVIGATION.md. GUI admite SearchGoToPosition, NavigateBack/Forward
y SearchHistoryPrevious/Next, además de flags existentes case/whole-word. Parser
line[:column]64B compartido con TUI; fuente escalar, no display/LSP. Go to captura
document/rev/encoding/ViewId/pane y revalida al editar/aceptar/pegar. No preview.
cancel_search_navigation abandona F3 diferido al iniciar otra intención modal sin
parar el scan. Historial sigue en Workbench; palette mueve el único borrador de
Replace y lo restaura en cancel/historia/flags. No clonar buffer/undo/drafts.

78 casos dirigidos aprobados,cuatro nuevos;14 checks nativos navigation,siete nuevos.
fmt/Clippy/release aprobados. El oráculo inicial TUI asumió Go to dentro de pin;
se corrigió: navegación automática va al otro grupo, historia opera en el activo.
No modificar PaneManager para volver verdadera una expectativa distinta.
Evidencia m2b-navigation-2026-11-03-*; Explorer y perfiles cerrados no se repitieron.

Continuar con paridad GUI de líneas/comentarios sobre comandos/metadatos existentes,
construcción amplia antes de checks. CI/push y proveedores/AVM no autorizados;
clipboard OS/IME/DPI/accesibilidad/RAM/pantalla al final, sin pedir manual ahora.

## Último cierre: líneas/comentarios GUI — 2026-11-03

Leer M2B_NATIVE_LINE_EDIT.md. Seis comandos existentes pasan por registro/keymap/
Workbench, sin otro motor. editor_input distingue edición de fuente de campos/
readmodels; Palette conserva invocador y compara doc/rev/encoding/ViewId/pane/
caret/anchor antes de aceptar. Comentarios vienen del perfil, sin condicional ABL
en frontend. Delete/Move son caretline, Duplicate usa selección si existe; EOF
vacío cuenta como línea. No asumir que mover a ese EOF conserva newline final.

23 casos dirigidos y14 checks HWND line-edit aprobaron a la primera (3/7 nuevos),
fmt/Clippy/release aprobados. Sin suites navegación/Explorer ni perfiles repetidos.
Evidencia m2b-line-edit-2026-11-03-* con fuentes/binarios y PNGGDI sin pérdida.
Sigue inventario de paridad universal GUI restante antes de proveedores opcionales;
no añadir AVM/regex/dependencias/CI/push. Manual/global al final; GNU notices y
symlink omitido siguen pendientes, no inferir certificación de distribución.

## Último cierre: selección y reapertura — 2026-11-03

Leer M2B_SELECTION_REOPEN.md. SearchSelectionNext/Previous, FileReopenClosed,
SearchCancel y Tab1…9 ya están en GUI teclado/menús/paleta. S0 literal conserva
flags/cuotas; decodedUTF8>4096 se rechaza antes de set_query. Un resultado Complete
vigente puede navegar sincrónicamente; test tardío debe iniciar Running nuevo.
try_reopen_closed informa IO/collision sin consumir metadata, bool wrapper compat;
no recuperar dirty descartado o undo destruido. MismoTabId, DocumentId nuevo;
new-file vacío, saved-file disco actual. No segundo buffer para path ya abierto.
close_flow reemplazó inline y revalida SourceEditTarget después de confirmación.

11 casos dirigidos y15 propios-HWND aprobados (4/8 nuevos), fmt/Clippy/release.
Evidencia m2b-selection-reopen-2026-11-03-*; dos fallos de arnés conservados, ninguna
suite cerrada/perfil repetido. Seguir capacidades universales GUI restantes de
selección/rectángulo antes de proveedores. No nuevos servicios/AVM/CI/push ni
preguntas manuales ahora; globales/físicos/notices GNU/symlink siguen pendientes.

## Último cierre: rectángulo GUI — 2026-11-03

Leer M2B_RECTANGLE_GUI.md. Seis comandos, AltShiftflechas y Alt+drag admiten solo
repertorio/cuotas existentes. VisualPosition/LineLayout/Workbench poseen geometría;
GUI/TUI con rect activo priorizan grid TAB4 (invisibles marca celdas, no escapes).
No clonar buffer/undo ni reconstruir offsets desde labels; clipboard rectangular
interno hasta nueva copia lineal, nada de clipboard rectangular OS implícito.

Drag usa SourceEditTarget con intención visual, ReleaseCapture fuera de préstamos,
solo HWND propio; loss/resize conserva selección, Escape cancela, stale no revive.
editor_input protege modales también en TUI; Undo/Redo/clipboard requieren Editor.
Flechas normales salen del rect incluso sin byte adyacente BOF/EOF. Una acción
efectiva termina rect y publica un undo; pin/vistas siguen F1.

26 dirigidos y15 HWND aprobados (4/8 nuevos), fmt/Clippy/release; evidencia
m2b-rectangle-2026-11-03-* retiene bugs/lint/compile iniciales y hashes. No repetir
suites/perfiles cerrados por docs. Sigue inventario universal GUI y gates M2;
manual/global al final. Sin dependencias/proveedores/AVM/CI/push nuevos, symlink
y notices GNU pendientes, no inferir certificación del ZIP o Unicode general.

## Último cierre: inventario GUI y edición/grupos — 2026-11-03

Leer [inventario y evidencias](M2B_GUI_INVENTORY.md) y matriz completa: 104 comandos,
85 con ruta real en GUI y 19 sin soporte directo, con motivo. Registro/predicados,
menús nativos y scope del binding no equivalen a todas las entradas validadas.
Paleta lista también deshabilitados. No volver a ejecutar suites históricas para
llenar filas; sus cierres y fechas están separados de la evidencia nueva.

Indent/Outdent están en Edit; Ctrl+Alt+L/View/paleta gobiernan el enlace de scroll
Workbench, con indicador persistente. Solo delta vertical, sin sincronizar caret o
pin. Indent/Outdent/Join/Split usan editor_input también en TUI; modal/Explorer y
targets de paleta cambiados no editan fuente oculta. Dos casos Rust dirigidos y
12 checks HWND aprobados (uno/cinco nuevos), fmt/Clippy/release. Fallo inicial de
Clippy en fixture conservado; funcionales pasaron a la primera, sin perfiles repetidos.

Próxima integración universal pendiente: host de panel inferior GUI, Problems/Output
F3 y foco de tres regiones. Primero concretar contrato de filas/destinos/estado y
cancelación; no implementar proveedores por inferencia. SearchResults ofrece panel
de proyecto, no el host genérico. Selectores aún no tienen Ctrl+V; Menu* usa adaptación
Win32 parcial, no OpenMenu TUI. Conservar todos los límites físicos/globales,
Unicode general/clipboard OS, symlink y notices GNU; sin CI/push/dependencias/AVM.

## Último cierre: host GUI Problems/Output — 2026-11-03

Leer [contrato y evidencia](M2B_BOTTOM_PANELS.md). TaskPanels posee filas/IDs/origen
únicos, reemplazando los logs y stringificación TUI. Ambos frontends aceptan F3
mediante API compartida; GUI separa Git y otros resultados en su único poll de
TASKS_MESSAGE. Preparación/dibujo solo leen resultados y preparan viewport visible.
JobRecord/CompletedTask llevan ruta capturada, cancel_stale y activación revalidan
stamp/ruta/fronteras. SaveAs puede invalidar aunque revisión y encoding no cambien.

Ctrl+J/View/paleta muestran host, Alt+1/2 cambia panel, F6/Shift+F6 recorre las tres
regiones visibles, Escape vuelve a Editor sin cancelar servicios. Problems navega
solo documentos abiertos vigentes por row ID; Output conserva origen pero nunca
navega/ejecuta/aplica propuestas. Raíz nueva cancela jobs/resetea logs; descarte
antiguo solo informa estado sin revivir payload. Quotas256KiB/log incluyendo
metadata/capacidades, texto4096B,1000/2000 filas; no RAM privada. Mouse IDs publicados
en WM_PAINT, geometría/retención verificadas, compact conserva selección del host.

23 casos Rust dirigidos (cuatro nuevos),13 HWND (seis nuevos), fmt/Clippy/release,
sin fallos funcionales. Native inicial retenido y final con origen visible/canvas;
Rust aprobados no repetidos por esos cambios GUI. Proveedores de prueba sintéticos
ejercitan JobSystem real, no AVM/negocio; sin productor el host queda vacío.
Matriz previa queda histórica;88 rutas GUI actuales. Otros pendientes del inventario
son entrada avanzada de selectores, menús y mouse general. No iniciar proveedores,
regex/dependencias/AVM/CI/push por inferencia ni repetir perfiles cerrados. Gates
físicos/globales/clipboard/Unicode, symlink y notices GNU permanecen abiertos.

## Último cierre: entrada de selectores GUI — 2026-11-03

Leer [contrato/evidencia](M2B_SELECTOR_INPUT.md). FieldSelection/edit_selected_field
comparten fronteras y operación de campo; walker común de present_field/selected
retiene solo <=32 filas visibles. GUI query256UTF8 sin controles/bidi: preflight
posterior al reemplazo conserva input/caret/anchor/candidato/revisión al rechazar.
Teclas/menú CopyCutPasteSelectAll del selector usan entry_context.field_input;
filas de paleta siguen contexto invocador para ejecutar Source, no ese campo.
CtrlV explícito nativo lee fuera del préstamo con salida máxima256UTF8; target
captura generación no reutilizada, intención, candidato, Source metadata/ruta,
invocador/encodingtarget/raíz/foco y se compara antes/después. Box solo en Action
de ese handoff; no agrandar todas las acciones. Copy/Cut vacío y PasteNone/empty
no cambian el campo. Nunca probar contra clipboard privado: usar transporte smoke.

Home/End ahora son del caret; CtrlHome/End primero/último candidato. UpDown/Page/
TabShiftTab recorren resultados. Escape conserva Search draft/Explorer invocador/
Bottom; Files/Encoding aceptados vuelvenEditor. QueryUnicode es independiente de
encoding de fuente. Sourcebytes/undo y reapertura/pin se mantienen en Workbench.
23 Rust distintos (24 pases por un decoder solapado), cuatro nuevos;13 HWND/seis
nuevos, fmt/Clippy/release aprobados. Logs conservan fallos de fixture/compile/lint;
native a la primera. No suites/perfiles cerrados repetidos por documentación.
El inventario conserva88 rutas; menús/mouse general y puertas previas pendientes.
Sin proveedores/dependencias/AVM/regex/instalaciones/CI/push/publicación ni seguridad.

## Último cierre: menús nativos y ratón fuente — 2026-11-03

Leer [contrato y evidencia](M2B_NATIVE_INTERACTION.md). NativeMenuGroup compone
HMENU desde default_menu_model/registro/handlers, sin IDs por rangos ni duplicados.
Seis macros Menu* y AltF/E/S/P/T/V/N/G/F10 tienen ruta real. Build/Help continúan
sin provider inventado. Native shortcuts/checks dependen del control; field clipboard
mantiene su destino y la paleta su invocador. Menú conserva Find al cancelar y
restaura su draft al abrir macros desde paleta; ese último camino no tuvo caso
HWND separado. MenuStamp valida fuente/campo/raíz/foco/generaciones/selección Explorer
antes de cerrar modales o actuar; popup/capture no retienen RefCell.

Drag lineal mantiene ancla/pane y usa hits escalares/diagnósticos/TAB4→bytes legales,
Shift extiende, sin autoscroll. PointerContext comparte rechazo stale con rectángulo.
Wheel usa coordenadas screen signed→client fuera del préstamo y configuración OS;
fracciones120 por contexto, sin foco/caret/selección nuevos. ViewState.manual_scroll
suspende reveal hasta intención explícita; enlace aplica delta consumido y pin es
independiente. Resize limita viewport. No restaurar paint::click ni create_menu
numérico ni introducir otro buffer/undo para los gestos.

16 Rust distintos/cuatro nuevos,13 HWND/seis nuevos, fmt/Clippy/release aprobados.
Construcción completa antes de pruebas, native a la primera; tres lints iniciales
del arnés retenidos. [Hashes y logs](evidence/m2b-native-interaction-2026-11-03-metadata.json),
[ZIP](evidence/m2b-native-interaction-2026-11-03-package.json). Ahora94 rutas/104 IDs,
no aceptación total. Histórico de inventario intacto. No repetir suites/perfiles
cerrados por documentación. Conservar bloques amplios y manual/OS al final.
Pendientes física/global/Unicode/clipboard/IME/DPI/accesibilidad, mouse horizontal/
autoscroll/word-selection/campos, symlink y notices; proveedores/terminal/regex
requieren sus propias puertas. Sin nuevas dependencias/AVM/CI/push/publicación.

## Último seguimiento: invocador de menú validado, gestos sin admisión

Leer [auditoría y evidencia](M2B_MENU_INVOKER_VALIDATION.md). El recorrido faltante
de171af05 paleta→Menu* con Find/Replace ya está validado: seis macros, tres campos
activos, cancel/repetición, continuidad de edición, acción permitida/deshabilitada
y ocho reentradas obsoletas. Un check nuevo + siete comunes, tres rechazos CLI,
fmt/ClippyGUI/releaseGUI aprobados a la primera. Sin ejecutar unitarias o repetir
secciones históricas, cambiar handlers o generar otro ZIP. Evidencia anterior se
conserva histórica; ya no falta esa combinación separada.

No activar mouse horizontal/autoscroll/selección por palabra/mouse en campos:
PLAN/AGENTS/contratos los describen como pendientes/excluidos, no orden concreta.
La decisión necesaria es admitir qué conjunto y definir gesto/coordenadas/límites/
cancelación antes de otro bloque. No ampliar el plan retrospectivamente para
justificar código ni inferir M3/proveedores. Se conservan las puertas física/global/
Unicode/clipboard externo/IME/DPI/accesibilidad/symlink/distribución; sin nuevos
permisos/dependencias/proveedores/AVM/terminal/regex/CI/push/publicación.

## Cierre local — resaltado compartido, 2026-11-03

Por prioridad explícita de Juan se integró resaltado léxico ABL, Rust, JSON y
JSONC en TUI y GUI; otros perfiles permanecen Plain. Contrato, límites, fallos
iniciales y evidencia: [M2_SYNTAX_HIGHLIGHTING.md](M2_SYNTAX_HIGHLIGHTING.md).
Pasaron35 casos Rust distintos (18 nuevos),11 checks HWND (4 nuevos),5 flujos
ConPTY,3 rechazos CLI, fmt, Clippy y build release locales offline. No se ejecutó
toda la suite runtime del workspace ni CI. No equivale a aceptación física/global.

El diff entre documentos abiertos queda suspendido y recuperable en
`checkpoints/open-diff-before-syntax-66aa783.zip` y su manifiesto; no está activo
ni validado. No restaurarlo automáticamente sobre el resaltado.

La autorización posterior del usuario permite incorporar y fijar una dependencia
Rust mantenida para regex real (candidato regex-automata) en el siguiente bloque
Search Plus. Sustituye la restricción anterior para esta dependencia concreta:
no pedir de nuevo permiso. No se añadió en este bloque, regex sigue deshabilitado
y no están autorizadas instalaciones de programas del sistema.

## Regex implementado — continuación TUI, 2026-11-03

Regex real de documento y proyecto está integrado con regex-automata=0.4.18,
autorizado por Juan. Alt+R, historial, rangos originales, cancelación/cuotas y
reemplazo literal por transacciones; [contrato y evidencia](M2_REGEX_SEARCH.md).
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

## Comparación de abiertos TUI implementada — 2026-11-03

El checkpoint suspendido de comparación sin Git ya se integró sobre regex y
resaltado actuales: controller con dos orígenes versionados, admisión dual F3,
presentación compartida acotada y visor TUI lateral/unificado. Comandos View/
paleta, hunks, navegación a grupo original, stale/refresh/cancel; fuente de solo
lectura y sin writes. [Contrato y evidencia](M2_OPEN_DOCUMENT_DIFF.md).
GUI de este visor sigue pendiente; no restaurar su checkpoint automáticamente.

54 casos distintos observados aprobados, cuatro flujos ConPTY, fmt/Clippy/release.
Las nueve regresiones Git pasaron con acceso autorizado a fixtures propios tras
fallar por acceso sandbox; no se usa Git en la comparación del producto.
No CI ni aceptación física/global. Siguiente bloque realmente activo: contrato
de capacidades internas opcionales y consumidor TUI de símbolos/navegación,
según EXTENSION_ARCHITECTURE y [matriz de cierre TUI](M2_TUI_FINISH.md).

## Capacidades locales de fuente TUI — 2026-11-03

Primer registro interno namespaced y proveedor opcional progress.abl operativo
para símbolos y referencias léxicas; core.words completa palabras textuales de
cualquier documento. Listas/selector TUI, entrega F3, stale, candidatos confinados
y propuestas de completado por revisión/una transacción. [Contrato y evidencia](M2_SOURCE_SERVICES.md).
40 casos distintos (17 nuevos), cuatro flujos ConPTY, fmt/Clippy/release locales
aprobados. No AVM, semántica, loader externo, GUI ni aceptación física/global.

Siguiente bloque activo: plantillas/snippets universales y catálogo ABL del
proveedor opcional, parámetros/preview/aceptación y undo único. Continuar sin
esperar otro «continúa»; construir el conjunto antes de su ventana de validación.

## Snippets TUI — cierre local 2026-11-03

Motor universal acotado con parámetros, preview y aceptación por revisión/una
transacción. progress.abl aporta siete plantillas declarativas de Insert/Surround;
sin reglas ABL en jc-app. Preserva encoding/BOM/EOL/undo; propuestas obsoletas se
rechazan. [Uso y evidencia](M2_SNIPPETS.md):38 casos distintos (11 nuevos), cuatro
recorridos ConPTY, fmt/Clippy/release aprobados. Sin AVM ni aceptación física/global.
Sigue auditoría final de teclado, foco y flujos TUI; CI permanece en M5.

## Auditoría de overlays/teclado TUI — 2026-11-03

Pegado y teclas respetan el overlay superior sobre snippets/completado/diff.
Rutas/prompts4096 UTF8, posición64 y paleta256; rechazo atómico al exceder cuota.
CtrlU vacía campos; Home/End/Page navegan menú/paleta sin perder selección visible,
Page en preview de reemplazo usa filas de la última presentación. Sin segundo buffer.

117 casos TUI distintos aprobados (cinco nuevos), entre corrida y cierre dirigido.
Dos expectativas históricas se actualizaron a regex habilitado y al nuevo final
del menú View; fixture de menú Help vacío corregido. Fmt/Clippy/release aprobaron.
Tres flujos ConPTY finales aprobaron,27557 bytes VT/salida0/fixture intacto.
Tres intentos iniciales quedaron esperando el título de la paleta, confundido con
una entrada del menú por el borde del snippet; se corrigió ese oráculo. El primer
intento nunca llegó a enviar bracketed paste: no acredita ni refuta ese transporte.
Event::Paste y sus cuotas sí están cubiertos por casos Rust; no clipboard OS real.

Siguiente bloque activo: adaptar clipboard Windows estricto a la TUI, solo por
acción explícita; separar validación inyectada de interoperabilidad física.

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

[Contrato y límites](M2_TUI_CLIPBOARD.md). Sigue layout Unicode general: Juan
autorizó unicode-segmentation=1.13.3 y unicode-width=0.2.2; [admisión](M2_UNICODE_LAYOUT_ADMISSION.md).
Esa autorización sustituye la restricción previa para estas dos dependencias;
no pedirla de nuevo. Sin instalación de programas, CI ni AVM.
