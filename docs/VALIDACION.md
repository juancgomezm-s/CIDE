# Validación

**Evidencia local más reciente:** Git read-only con tareas versionadas y visor TUI,
319 pruebas distintas aprobadas al integrar Join/Split sobre el rectángulo previo
(cierre dirigido; no una corrida única de 319),
fmt/Clippy/release, tres pares fríos equivalentes y catorce procesos nativos
retenidos de edición/undo en ocho documentos. Máximo histórico actual 52,461 MiB;
Memoria/teclado global y
revisión renderizada GUI pendientes. Véase el bloque final; las secciones
anteriores conservan sus fechas y limitaciones históricas.

Estado de la primera base preparada antes de publicarla:

- Workspace Rust compilado localmente.
- Pruebas de edición por bytes, undo/redo, perfiles de lenguaje y protocolo Progress ejecutadas en el entorno de desarrollo.
- `cargo fmt`, `cargo test`, `cargo clippy -D warnings` y build release usados como comprobaciones de la base original.
- Se probaron bytes no UTF-8 y CRLF en la CLI headless.

## M1 — Archivo y texto interpretado

La corrida de cierre funcional previa al commit documental fue GitHub Actions **#30 / run 41269634082**. En Ubuntu y Windows completó con éxito:

- `cargo test --workspace --locked --offline`: **67 pruebas**, 0 fallos;
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`;
- build release del workspace;
- demo headless.

La matriz incluyó tests específicos de Windows para el ciclo de guardado que utiliza `ReplaceFileW`, además de creación no-clobber, fallo de escritura, conflicto externo, codecs legacy, UTF-8/BOM, line endings, byte↔posición y límites/compactación del historial.

La validación final del estado guardado se ejecuta nuevamente con `cargo fmt --check` estricto después de fijar el resultado de rustfmt.

## Política de validación desde M2

Las corridas de GitHub Actions registradas arriba son **evidencia histórica de M0/M1**, no la política de desarrollo para las épicas siguientes.

Por decisión D13, desde M2 hasta completar M4 no se ejecutará CI remoto automáticamente ni se utilizará GitHub Actions para corregir iterativamente errores introducidos mientras la arquitectura sigue cambiando. Las pruebas se ejecutan localmente y de forma dirigida cuando el entorno lo permite. Los huecos de validación se documentan y el hardening transversal queda concentrado en M5.

## M2A — Desacoplamiento multilenguaje

Se añadió a los 18 perfiles de `jc-languages` la capacidad declarativa de delimitadores de comentario y se retiraron del `jc-app` las tablas/casos específicos de nombres de lenguajes para comentar. Los tests de perfil se ampliaron para ABL, C#, XML, Python, JSON y PowerShell.

**Estado de validación:** cambios revisados estáticamente; compilación y pruebas Rust aún no ejecutadas en este entorno. GitHub Actions permanece deshabilitado para pushes y PRs hasta M5 por la decisión D13. No anunciar esta modificación como validada en Windows o Linux.

## M2A — Checkpoint inicial doble panel / navegación (histórico)

Se agregó código de `PaneManager` más integración inicial en `Workbench`: grupos Primary/Secondary, referencia anclable, back/forward independientes, scroll link como estado y compartición de `TabId`. Se añadieron seis pruebas de modelo y cuatro de integración (aún sin ejecución).

**Estado de validación:** cambios preparados y revisados estáticamente; **no se ha compilado, formateado ni probado** el checkpoint en esta sesión. Debe verificarse localmente en Windows con `cargo fmt --all -- --check`, `cargo test -p jc-app` y `cargo check -p jc-tui`. La UI de doble panel, navegación ABL por código, autocompletado, diff/merge y Git completo siguen pendientes. GitHub Actions no se utilizará antes de M5.

## M2A — Doble panel TUI: validación local del 2026-11-01

Entorno: Windows del equipo del usuario, Rust **1.85.1 MSVC** y Build Tools 2022. El toolchain y la caché Cargo se prepararon en `work/toolchain`, sin modificar el PATH. Dependencias descargadas con `cargo fetch --locked`; después la validación se ejecutó offline. No se añadió ninguna dependencia de terceros: `jc-app` declara ahora su dependencia interna de `jc-core` para consumir la geometría `ByteEdit`.

Resultados satisfactorios, mediante `scripts/cargo-local.ps1`:

- `cargo fmt --all -- --check` en todo el workspace.
- `cargo test -p jc-app -p jc-core -p jc-tui --locked --offline`: **78 pruebas, 0 fallos** (47 de jc-app, 18 de jc-core y 13 de jc-tui, incluidas las del binario).
- `cargo check -p jc-tui --locked --offline`.
- `cargo clippy -p jc-app -p jc-core -p jc-tui --all-targets --locked --offline -- -D warnings`.
- `cargo build -p jc-tui --locked --offline` (debug).

En PowerShell se pasa el separador como `'--'` al wrapper, por ejemplo `scripts/cargo-local.ps1 clippy -p jc-tui --all-targets --locked --offline '--' -D warnings`.

Regresiones nuevas: selección/scroll independientes al cambiar foco, mismo documento/undo en dos grupos, rebase UTF-8 al editar/deshacer/rehacer, geometría inversa de undo con historia ramificada, scroll vinculado sin mover el otro caret, pin/apertura lateral/cierre compartido, reapertura sin duplicar una sesión sucia, eventos de teclado/palette y render amplio/estrecho. El menú View conserva visible el comando seleccionado en 45×10 y los overlays tienen fondo opaco.

Se ejecutó además el binario real en una terminal Windows/ConPTY de 80×24 con README y PLAN: apertura de menú con Alt+V, selección de Split Right, Focus Group 2 y Pin, Ctrl+O hacia un archivo ya abierto redirigido al grupo no anclado, Alt+Left, regreso al grupo de referencia y salida por File/Exit. La referencia conservó ruta y pin. A ese ancho con sidebar solo cabe el grupo activo; la presentación simultánea de dos columnas y la selección en terminales anchas están cubiertas por pruebas de eventos/render, no por una matriz manual completa.

Las primeras corridas dentro del sandbox devolvieron errores de permisos al usar hard links/`ReplaceFileW`. Mover los temporales al proyecto no eliminó toda la restricción; las pruebas finales se ejecutaron con esas primitivas permitidas y temporales en `work/test-temp`, y pasaron. No se cambió el algoritmo de guardado ni se omitieron pruebas para ocultar esos fallos.

**Límites del incremento:** no valida Linux, release del workspace, todas las terminales, ancho Unicode/grafemas, rendimiento, AVM/BD, resolver ABL, Outline, Git/diffs ni autocomplete. Aún falta búsqueda de proyecto y revisión manual exhaustiva para cerrar M2A. **Cero invocaciones de GitHub Actions; workflow sin cambios.**

## Auditoría de arquitectura y ecode — 2026-11-01

Base local: `d5eccfc`. Se revisaron fuentes de `ecode/eepp` fijadas por commit y rutas de core/texto/aplicación/TUI de JC-IDE. La [auditoría](AUDITORIA_ECODE_ARQUITECTURA.md) contiene enlaces, prioridades y límites; no se compiló ecode ni se comparó su rendimiento.

Se añadió y ejecutó `cargo run -p jc-app --example audit_baseline --release --locked --offline` mediante el wrapper local, Rust 1.85.1 MSVC. Reproducciones observadas:

- Replace-all de `a a` por `b b` seguido de un undo deja `a b`.
- Dos Right en `a\r\nb` dejan cursor en byte 2, sin posición lógica; jump a ese byte también es aceptado.
- Cerrar descartando y reabrir deja `document_dirty=true` y `tab_dirty=false`.
- Con historial cero, 100 reemplazos de un documento de 64 KiB retienen 6,553,600 bytes añadidos; la medición es retención del buffer, no RAM total.
- Baseline sintético de 4 KiB a 4 MiB: en 4 MiB, movimiento p50/p95 36.929/43.371 ms y consulta de posición 62.662/77.625 ms. Metodología y tabla completa en la auditoría; no incluye render ni carga real del usuario.

Comprobaciones de esta entrega: `cargo fmt --all -- --check`, `cargo clippy -p jc-app --example audit_baseline --locked --offline -- -D warnings` y ejecución release del ejemplo, satisfactorias. El ejemplo informa fallos conocidos y tiempos; no es un test que los considere correctos. No se modificó lógica de producción ni se presenta como reparada: esta entrega corrige el plan y establece evidencia para F1–F3.

No se volvió a ejecutar la suite de 78 pruebas del checkpoint al cambiar únicamente documentación y el ejemplo opt-in; su resultado anterior se conserva como histórico. No se ejecutó GitHub Actions.

## Límites actuales


No se ha ejecutado una AVM de Progress en este entorno. Los helpers ABL son experimentales hasta compilarlos/probarlos con las versiones objetivo.

No hay GUI todavía. No se ha medido memoria de una aplicación gráfica, tiempo de arranque, IME, accesibilidad ni latencia completa con archivos grandes. La medición sintética anterior cubre solo dos operaciones del modelo hasta 4 MiB.

Los presupuestos del plan son objetivos pendientes. Los números del baseline son observaciones limitadas, no evidencia de que esos objetivos se hayan alcanzado.


## F1 — Validación local del 2026-11-01

Implementación sobre el checkpoint de auditoría `d7dcb69`, Windows y Rust 1.85.1 MSVC. Comandos mediante `scripts/cargo-local.ps1`, dependencias offline:

- `cargo test --workspace --locked --offline`: **127 pruebas, 0 fallos** (jc-app 55, jc-core 21, jc-tui 14, jc-cli 6, jc-files 10, jc-languages 4, jc-progress 9, jc-text 8).
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: satisfactorio.
- `cargo build --workspace --release --locked --offline`: satisfactorio.
- `cargo fmt --all -- --check` y revisión de whitespace Git: satisfactorios.

Hay 12 regresiones nuevas: 3 de transacciones core, 8 de F1 en aplicación y 1 de apertura legacy TUI. Se conservó la suite anterior y se actualizó el caso de cierre compartido para exigir supervivencia de la otra vista. Las regresiones prueban contenido e historial frente a bytes planos, lotes rechazados sin cambios, undo único, mapas inversos/ramas, revisión obsoleta, CRLF, UTF-8/BOM, encoding estricto, documento inactivo, selección/scroll por vista, cierre y descarte/reapertura.

Las pruebas de guardado se ejecutaron con hard links/ReplaceFileW permitidos y temporales del proyecto. Cargo fix encontró inicialmente una restricción del sandbox en su socket local; se ejecutó después con esa comunicación permitida para aplicar únicamente las sugerencias de mutabilidad innecesaria. No se alteraron pruebas ni primitivas de guardado para sortear la restricción.

El [checkpoint F1](F1_DOCUMENT_VIEWS_TRANSACTIONS.md) describe cambios y política de posiciones. Los fallos de undo/CRLF/dirty/prompt de la auditoría son históricos y ahora están cubiertos por regresiones. Sigue pendiente la optimización F2: no se certifican RAM ni latencia, ni se atribuye mejora de rendimiento por las pruebas de corrección. Render puro/jobs corresponden a F3; GUI, Linux, matriz manual de terminales, AVM y restauración exacta de selección histórica no se validan con esta entrega. Cero invocaciones de GitHub Actions.


## F2 — Validación local de rangos, índices y memoria

Implementado sobre F1 `ddc9d41`, Windows 11 Pro 10.0.26300, Ryzen 9 8945HS (8/16), Rust 1.85.1 MSVC. Dependencias offline y wrapper local.

- `cargo test --workspace --locked --offline`: **142 pruebas, 0 fallos**: jc-app 63, jc-core 26, jc-tui 15, jc-cli 6, jc-files 10, jc-languages 4, jc-progress 9, jc-text 9.
- `cargo fmt --all -- --check`, Clippy de workspace/all-targets con `-D warnings` y build release del workspace: satisfactorios.
- Ejemplos release locales `range_layouts`, `f2_text`, `audit_baseline`: ejecutados; datos originales guardados en `docs/benchmarks/`.

Las nuevas regresiones comparan segmentos con slices planos y la proyección incremental con el decoder/índice completos después de byte edits, lotes y undo/redo; incluyen UTF-8 inválido y los cuatro encodings, checkpoints dispersos en líneas largas, cuota de caché bajo presión, recolección sin pérdida de estados retenidos, presupuesto por bytes y transiciones grandes ya descartadas. Render de dos vistas consume sólo rangos visibles sin escaneo global; fragmentos BOM conservan offsets absolutos.

La primera matriz ampliada detectó reconstrucción completa del layout de una línea larga tras editar. Se sustituyó por actualización local de checkpoints y se agregaron regresiones específicas antes del cierre. Método, cifras finales y límites en [F2_RANGES_INDEX_MEMORY.md](F2_RANGES_INDEX_MEMORY.md). Los 131,072 bytes añadidos finales del diagnóstico sin historial son retención de buffer, no RAM total.

Las pruebas de archivo se ejecutaron con hard links/ReplaceFileW permitidos y temporales del proyecto. No se usó CI remoto. No se valida aquí Linux, nueva matriz manual de terminales, GUI, IME/grafemas, Progress/AVM ni latencia completa de UI. Los objetivos globales de RAM/arranque continúan abiertos; F3 sigue pendiente.

## F3 — Presentación y tareas: validación local del 2026-11-02

Base `e72856f`, rama `m2a/dual-pane-tui`, Windows 11 Pro 10.0.26300,
Rust 1.85.1 MSVC aislado en `work/toolchain`. No se añadieron dependencias de
terceros, no se cambió el perfil release ni se invocó GitHub Actions.

Comandos satisfactorios, mediante `scripts/cargo-local.ps1`:

- `cargo fmt --all -- --check`.
- `cargo test --workspace --locked --offline`: **157 pruebas, 0 fallos**.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`.
- `cargo build --workspace --release --locked --offline`.

Quince pruebas nuevas cubren preparación/composición sin cambios de vistas/caché,
terminal incremental, no-op/cursor, borrado de texto/estilos, resize e invalidación
por fallo; cuota/cierre de cola de entrada; snapshot inmutable durante edición,
revisión/encoding/vista/reapertura, generaciones, presión de cola, límites de
resultados, cancelación/deadline/panic, logs y entrega a paneles sin otra tecla ni
aplicación automática de propuestas. Las pruebas F1 de vistas y transacciones,
F2 de rangos y las de archivo/encoding volvieron a pasar. Los tests de archivos
se ejecutaron con hard links/ReplaceFileW permitidos, temporales en el proyecto.

Una segunda corrida completa devolvió error Windows 1175 en `force_save` de
`external_content_change_blocks_normal_save_and_preserves_external_version`
(ReplaceFileW: no se puede quitar el archivo para reemplazarlo). La repetición
dirigida de las ocho pruebas de sesión pasó sin cambios. Es una incidencia
intermitente de causa aún no identificada; no se atribuye a OneDrive, antivirus
o sandbox sin evidencia ni se modificó el guardado para ocultarla.
La siguiente corrida completa del árbol final pasó las 157 pruebas, 0 fallos.

Benchmarks release opt-in, Ryzen 9 8945HS, 8 núcleos/16 hilos, RAM visible
16.037.152 KiB, perfil existente de tamaño con thin LTO/un codegen unit:

- `f3_pipeline`: Right → preparación → composición → VT a memoria, 5 muestras de
  calentamiento +31 medidas por tamaño/viewport; 4 KiB/1 MiB/4 MiB, 80×24/140×40.
  p95 35–97 µs; delta mediano 23/41 bytes; frame idéntico 0 bytes. Excluye entrada
  OS, transporte y pantalla. [CSV](benchmarks/F3-pipeline.csv).
- `f3_jobs`: 5+31 muestras, proveedor de mensaje y proveedor cooperativo que consulta
  token entre sleeps de 1 ms. Admisión-a-entrega p50/p95 12/20 µs; cancelar trabajo
  iniciado-a-entrega 1537/1567 µs. No mide proveedor real ni búsqueda.
  [CSV](benchmarks/F3-jobs.csv).
- Ejecutable real mediante `scripts/smoke-conpty.ps1` y ejemplo Windows opt-in
  `f3_console_host`: fixture ASCII de 145 bytes, 80×24, 3 Right de calentamiento y
  31 medidos. Inyección-a-salida VT p50/p95 **15,7802/16,0550 ms**; 1150 bytes,
  ningún clear en teclas, un clear tras resize a 120×30 y salida File/Exit código 0.
  Proceso editor: 1.089.536 bytes privados y 0 ms CPU en una ventana de reposo
  de 250 ms. Excluye bootstrap/conhost/anfitrión. [JSON](benchmarks/F3-conpty.json).

El primer intento del host heredó stdout redirigido de PowerShell y no capturó la
pantalla; se corrigió el host opt-in enlazando CONIN$/CONOUT$ de su pseudoconsola.
Otro intento usó End en un menú que navega con Up/Down; la prueba final sale con
Up desde la primera entrada. No se contabilizan esos intentos como validación.

**Límites:** no se midió el refresco/pintado de pantalla física ni arranque. El p95
ConPTY no cierra la meta tecla-a-pantalla <16 ms. El muestreo de RAM/CPU corresponde
a un fixture mínimo y proceso sin servicios; no acredita RAM global <50 MiB,
varios documentos, Unicode/grafemas, doble grupo nativo ni reposo universal. No es
matriz Linux/terminales/DPI/IME ni aceptación GUI. Cancelación es cooperativa;
un proveedor que ignore token puede bloquear su worker/join. La base actual es
por documento; identidad de proyecto, escaneos por lotes, Search Plus de proyecto
y proveedores Progress reales aún requieren integración. Search activo y Explorer
siguen síncronos. M2A continúa abierto; AVM M3, CI/Git con escritura M5 y plugins
externos post-M5. Detalle de APIs, cuotas, método y próximos pasos en
[F3_PRESENTATION_TASKS.md](F3_PRESENTATION_TASKS.md).

## Guardado Windows — continuación del 2026-11-02

Se implementó tratamiento acotado de 32/33/1175 con verificación de huella entre
intentos, conflicto si el destino cambia y conservación del temporal ante 1176/1177.
La causa del handle externo histórico no se identificó. La corrección acredita
el tratamiento seguro de esos errores, no la eliminación de bloqueos del sistema.

Validación local: **161 pruebas, 0 fallos**; fmt, Clippy workspace/all-targets con
-D warnings y build release satisfactorios. Cuatro regresiones nuevas: 1175
inyectado con commit nativo posterior, bloqueo persistente con cuota, conflictos
entre intentos, conservación ante fallos parciales, y lector nativo liberado/detenido
(las primeras cubren varios escenarios dentro de cada prueba).

Probe nativo de 300 guardados antes/después: el código 1175 se observó 1 vez antes
y 0 después en el escenario de mapeo; lector persistente produce 100 fallos seguros
con código 32 en ambas corridas. No se omiten ni se reclasifican como guardados.
Detalles, límites y fuente primaria: [SAVE_WINDOWS_CONTENTION.md](SAVE_WINDOWS_CONTENTION.md).
Datos crudos en benchmarks/SAVE-contention-before.csv y SAVE-contention-after.csv.
Cero CI remoto y ninguna modificación de servicios/configuración del sistema.

## Search Plus de proyecto — 2026-11-02

Sobre el guardado `ed51143`: búsqueda literal exacta con globs/exclusiones, worker
bajo demanda, snapshots abiertos, resultados por lotes versionados, cancelación y
navegación. Reemplazo con preview agrupado, confirmación explícita, un archivo por
turno y conflictos individuales. Abiertos usan un undo sin guardarse; cerrados usan
FileSession. No-op no reescribe. No hay transacción global del filesystem.

Validación Windows local: **174 pruebas, 0 fallos** (jc-app 82, jc-core 26,
jc-files 14, jc-tui 24, jc-cli 6, jc-languages 4, jc-progress 9, jc-text 9).
Fmt/check, Clippy workspace/all-targets con -D warnings y build release completos.
Trece nuevas regresiones respecto a ed51143 incluyen diez de integración de
proyecto, dos TUI y una verificación nativa de handle. Cubren bytes/encodings,
BOM/CRLF, raíz/junction/handle, globs, presupuestos, fuentes sucias/obsoletas,
generaciones, cancelación, preview sin escrituras, confirmación, undo, no-op y
conflictos. Una junction creada tras el preview también se rechaza sin alterar
el archivo externo. Las pruebas de guardado siguen presentes y pasaron.

Benchmark release project_search_bench: corpus ASCII de 2.097.144 bytes en cuatro
archivos, 64 coincidencias, cache de disco caliente, 5+31 muestras. Primera entrega
p50/p95 5707/7220 µs; fin 21239/27457 µs. Cancelar y hacer join después de la primera
entrega de una consulta densa: 56/73 µs. No mide terminal ni escrituras de reemplazo.
Datos en [S1-search.csv](benchmarks/S1-search.csv).

El ejecutable real pasó scripts/smoke-conpty.ps1 -ProjectSearch: carpeta/filtros,
resultado y completion sin otra tecla, navegación, preview cancelado, aplicación
confirmada, buffer abierto no guardado y un undo. El host configura UTF-8 solo en
su ConPTY privada y ahora interpreta movimientos relativos/borrado VT. La primera
prueba de undo se hizo con foco Output; se corrigió el recorrido para volver al
Editor, conforme al scope real de Ctrl+Z. No se cambió el keymap para acomodar
el test. Datos en [S1-conpty.json](benchmarks/S1-conpty.json).

Control previo de cursor: p50/p95 15,6115/16,0598 ms hasta VT, 1150 bytes/31 Right,
0 clears por teclas y 1 por resize; exit 0. Muestreo mínimo anterior al recorrido:
1.142.784 bytes privados del editor y 0 ms CPU en 250 ms, sin contar conhost/host.
Esto no acepta RAM global <50 MiB ni latencia hasta pantalla física <16 ms.

Límites: S1 es literal/sensible a mayúsculas; Unicode case folding completo,
regex real y migración acotada del scan de documento S0 siguen pendientes. No hay
aceptación Linux/GUI/DPI/IME/grafemas ni corpus exhaustivo. Cancelación cooperativa:
I/O del sistema y el commit de un archivo no son preemptibles. Las verificaciones
de huella/raíz no son un CAS ni aislamiento completo frente a un actor que cambie
nombres durante el commit. M2A permanece abierto. Detalle en
[S1_PROJECT_SEARCH.md](S1_PROJECT_SEARCH.md). Sin CI remoto, push ni cambios de
servicios, Qwen/GPU o configuración del sistema.


## S0 literal Unicode — cierre por bloques del 2026-11-02

Sobre `6e07b1e`: retirado el scan síncrono ASCII y el matcher de bytes separado de
proyecto. Un motor literal de fragmentos sirve a ambos. S0 admite default full
case folding Unicode 17 C+F, palabra Unicode, búsqueda de selección, historial,
marcadores e ir a posición escalar. Datos upstream fijados por hash, licencia
retenida, generación offline; sin crate ni proceso de lenguaje nuevo.

Al cierre del bloque integrado se ejecutó el workspace completo: **186 pruebas,
0 fallos**, fmt, Clippy workspace/all-targets -D warnings y release. Luego la revisión
de vistas añadió dos regresiones: Find/Replace Current respetan la vista activa y
una navegación diferida no pisa una intención posterior de caret/anchor/ViewId.
Validación final dirigida: S0 9, F1 8, Workbench 18 y TUI 26, todas aprobadas;
Clippy/release y recorridos Windows repetidos satisfactoriamente. **188 casos
distintos** comprobados; no se afirma una corrida completa de 188 ni se repitieron
suites F2 y mediciones que no cambian con esa corrección. Se aplica la preferencia
del usuario: construcción por bloques amplios, validación al cierre y comprobaciones
intermedias solo ante bloqueantes/revisiones materiales.

Las pruebas incluyen las 1585 entradas de folding C/F, word properties, oráculo
plano independiente frente a fragmentos 4/5/7/16, límites de expansiones escalares,
UTF-8 inválido/BOM/CRLF, páginas explícitas/encoding estricto, cuotas/deadlines,
cancelación que persiste al editar, undo y flujos de teclado. Fuente/encoding/EOL
permanecen canónicos; no hay snapshot completo por tecla ni worker que edite.

Mediciones release del mismo equipo/toolchain/perfil de S1: 5+31 consultas sobre
líneas ASCII. En 16 MiB, programar p50/p95 2/3 µs, slot p95 417 µs, fin
352732/401671 µs y cancelar después de un turno p95 1 µs. Proyecto compartido,
cuatro archivos/2.097.144 bytes/64 matches: primer lote 7815/9941 µs, fin
30633/37057 µs y cancel+join 48/65 µs. p95 de fin de proyecto **~35 % mayor** que S1
(27457 µs): coste registrado, pendiente optimizar decoder/scratch sin motores
paralelos. Datos en S0-document.csv y S0-project-shared.csv bajo benchmarks/.

ConPTY real UTF-8: STRASSE encuentra Straße sin otra tecla, F3 navega, Ctrl+G usa
escalares y reemplazo/undo no guardan el buffer. También pasó otra vez el flujo
S1 de proyecto/preview/confirmación. Datos versionados S0-conpty.json y
S0-project-conpty.json; reconfirmación final en work/S0-final-*.json. El control
de cursor previo dio p95 16,0726 ms hasta VT, no pantalla física. Memoria privada
muestreada en fixture mínimo antes del recorrido: 1.122.304 bytes; no RAM global.
Peso del ejecutable en S0-binary.csv; tamaño no equivale a RAM.

PowerShell presentó retraso/bloqueo al inicializar FileSystem. Se continuó con
Cargo directo, el mismo toolchain y variables solo del proceso, y el mismo host
C# mediante csc existente. No se modificaron servicios ni configuración global.
Helpers de junction ahora usan mklink /J con rutas propias entrecomilladas.

Regex continúa deshabilitado y no existe motor ni checkbox ficticio. No se valida
NFC/NFD/Turkic, UAX #29 por idioma, IME/grafemas, Linux, GUI/AVM ni corpus exhaustivo.
Los límites cooperativos no son preempción dura del allocator/decoder. RAM global,
cold peaks, latencia hasta pantalla física y aceptación completa M2A siguen abiertas.
Detalle y siguientes bloques en [S0_DOCUMENT_SEARCH.md](S0_DOCUMENT_SEARCH.md).
Sin push ni CI remoto; Qwen/GPU y configuración del sistema intactos.


## S0 — Optimización del decoder compartido — 2026-11-02

Sobre `570a6eb`: decoder prestado de escalares/rangos/validez en jc-text, reutilizado
por decode/decode_fragment y el único scanner documento/proyecto. Un buffer de
fuente retenido por tarea sustituye asignaciones por fragmento; rechazo ASCII sin
prefijo dentro del mismo matcher. Sin dependencias, cambios de perfil/cuotas ni
funcionalidad nueva. Se mantienen bytes, Unicode 17, posiciones, revisiones y
transacciones. [Implementación y mediciones](S0_DECODER_OPTIMIZATION.md).

Validación final local offline: **191 pruebas, 0 fallos**, fmt --check, Clippy
workspace/all-targets con -D warnings, release del workspace y host ConPTY. Las
tres regresiones nuevas comprueban bytes inválidos/truncados/BOM/rangos, todos los
bytes válidos de las tres páginas legacy y 400 matches plegados con reanudación,
posiciones y lotes acotados. Oráculos Unicode y regresiones de archivos/vistas pasan.
Se construyó el bloque antes de validar; una regresión material de throughput en
documento motivó el ajuste ASCII y la comprobación final, no microajustes repetidos.

Mismo corpus de proyecto (4 archivos, 2.097.144 bytes, 64 matches), 5 calentamientos
y 31 muestras, mismo Rust/perfil/hardware: primer lote p50/p95 antes 8262/10611 µs,
después 4697/5044 µs; fin 31225/40930 → 17587/18649 µs; cancelar+join
44/97 → 58/104 µs. Fin p95 baja 54,4 % contemporáneo y queda 32,1 % bajo S1
histórico (27457 µs) en esta carga. Cancelación sin mejora consistente.
Documento 16 MiB: schedule p50/p95 2/3 µs, slot p95 650 → 180 µs, fin
358788/632051 → 89093/178829 µs; cancelación p95 1 → 7 µs. Dispersión elevada,
sin aislamiento global; no son tiempos tecla-a-pantalla. CSV antes/después y primera
pasada en benchmarks/S0-decoder-*.csv; método completo en el documento del bloque.

Ambos flujos nativos finales aprobaron: STRASSE/Straße sin otra tecla, F3, Go to
escalar, reemplazo sin guardar y undo; proyecto con entrega/preview/confirmación y
undo. Datos en S0-decoder-document-conpty.json y S0-decoder-project-conpty.json.
Control de cursor hasta VT p95 16,0099/16,0911 ms, no pantalla física. Muestreo
privado mínimo 1.138.688/1.146.880 bytes previo al flujo, no RAM global ni peak.

No hay contador global de asignaciones/RAM, aceptación Linux/GUI/IME/grafemas,
regex/AVM ni cierre M2A. Sigue pendiente ampliar corpus Unicode/legacy/frío,
líneas extremas/muchas piezas y teclado, y aceptar RAM/latencia físicas. Sin CI
remoto, push ni cambios a Qwen/GPU, servicios o configuración del sistema.


## M2 — Corpus ampliado y memoria de procesos — 2026-11-02

Sobre `282d006`, mismo Windows/Rust/perfil release. **194 pruebas completas,
0 fallos**, fmt --check, Clippy workspace/all-targets -D warnings y release.
Tres regresiones nuevas: layout frío sobre bordes 64 KiB/UTF-8 inválido/BOM/EOF;
corpus Unicode/legacy con rangos/coordenadas, fragmentación y reemplazo/undo;
teclado de edición y Tab/BackTab fuera del editor en cuatro páginas.

Se corrigió un defecto medido: layout inicial decodificaba toda la línea y creaba
un offset por escalar. Ahora recorre rangos de 64 KiB/contexto mediante el decoder
compartido y solo conserva checkpoints. Una línea ASCII de 16 MiB llevó al editor
real a peak commit 456.040.448 bytes antes y 18.833.408 después (434,9 → 18,0 MiB).
Caso UTF-8 del modelo: primera proyección 100,04 → 73,77 ms y peak live solicitado
117.444.841 → 17.371.537 bytes. Frío de modelo tiene una muestra por caso, no p95.

Ejemplo opt-in con allocator instrumentado, ocho corpus nuevos en procesos frescos:
4 MiB ASCII/Unicode/tres páginas legacy; línea Unicode 16 MiB; 4097 piezas; ocho
documentos de 4 MiB con líneas de ocho bytes. Operaciones calientes 5+31 muestras
crudas: p95 Right+posición+viewport de dos grupos 53–82 µs según carga; fuentes,
historial/proyección, asignaciones y proceso medidos por separado. No mezclar
tiempos instrumentados con S0 sin wrapper. El caso denso de ocho documentos sigue
en 72.847.360 bytes privados (~69,5 MiB) y baja a 1.216.512 tras cerrar; objetivo
global de 50 MiB abierto. Recolección de fuentes de la línea larga aún provoca
peak commit del modelo de 35.049.472 bytes. No se ocultan otros picos/retención.

ConPTY: 24 sesiones antes y 24 después, base/8 documentos/una línea 16 MiB, con
un proceso de calentamiento y siete retenidos por forma. Arranque hasta primer
Right observado p95 final 63,95/114,44/203,64 ms; no caché del SO vaciada.
Dos vistas y 31 Right retenidos por sesión, selección Alt+1..8, One Group,
Ctrl+W de todos y File/Exit. Memoria editor/host Rust/conhost/controlador C#
registrada aparte. Suma máxima de private contemporánea editor+host+conhost
final: 3,61/41,16/20,24 MiB. Los procesos auxiliares son del test/transporte,
no servicios de lenguaje. Progress/LSP no se iniciaron.

Inyección-a-VT p95 final 16,084/16,160/16,048 ms: no pantalla física, ni prueba
de la meta <16 ms. Otra vez pasó el recorrido Unicode nativo de búsqueda sin otra
tecla/F3/Go to/reemplazo sin guardar/undo. Raw samples y método en
[M2_ACCEPTANCE_EVIDENCE.md](M2_ACCEPTANCE_EVIDENCE.md) y benchmarks/M2-evidence-*.

La primera tentativa del nuevo host a 80 columnas no pudo observar Col por la
longitud de ruta; no se usó como medición. Se corrigió el arnés a geometría 140×40
y rutas relativas, sin cambiar el producto. Los CSV retenidos son corridas completas.
La granularidad de muestreo solicitada es 5 ms, dependiente del planificador;
peak commit del SO complementa picos privados muestreados, no son el mismo dato.

Pendientes: metadatos densos O(L), distribuciones frías, caché fría real, más
teclado/IME/DPI/portapapeles, Linux, GUI y pantalla física. No se declara cierre
M2A ni <50 MiB global. Sin CI/push/instalaciones/configuración/servicios/Qwen/GPU.


## M2 — Dos frentes paralelos autorizados — 2026-11-02

Juan autorizó varios workers. Capacidad nativa anunciada: cuatro agentes total;
se usaron tres, incluido el integrador. `/root/diff_read_model` tuvo propiedad
exclusiva de diff_model.rs/DIFF_MODEL_READ_ONLY.md y `/root/gui_scene_study` de
prototypes/m2b/*/M2B_SCENE_STUDY.md. El integrador conservó layout/aceptación,
exports/APIs compartidas/plan/VALIDACION/commits. Sin chats principales nuevos,
worktrees externos ni simulación de workers mediante procesos. Los benchmarks
de aceptación terminaron antes de admitir estos frentes. Los workers avanzaron
diseño/código y comprobaciones ligeras; Cargo y las suites se ejecutaron en serie.

DiffModel puro prestado: fuentes inmutables de bytes, líneas tipadas, Myers,
hunks/correspondencias solo iguales, cuotas de bytes/líneas/trabajo/metadatos/hunks,
cancelación cooperativa también durante comparación de líneas largas. Un resultado
limitado/cancelado no publica equivalencias parciales. No recibe Document/sesión/
undo, no normaliza bytes, no propone cambios ni llama Git. Seis pruebas nuevas,
incluido oráculo LCS independiente sobre 961 pares pequeños, EOL/BOM/invalid,
newline final, correspondencias, cuotas y cancelación. Detalle en
[DIFF_MODEL_READ_ONLY.md](DIFF_MODEL_READ_ONLY.md). No tiene todavía UI/proveedor
Git, integración con jobs/versiones ni aceptación de rendimiento de diff.

La integración pasó la **suite completa de 200 pruebas, 0 fallos**, fmt --check,
Clippy workspace/all-targets -D warnings y release. Clippy pidió enumerate en
un bucle del oráculo: corrección solo de test, seguida de seis pruebas DiffModel,
Clippy y fmt aprobados; no se repitieron las suites/producto/benchmarks inalterados.
Las mediciones M2 anteriores no se atribuyen a un motor diff ni a un backend GUI.

Estudio GUI HTML/CSS/JS local: fuente ABL, include PIN, Explorer, Search y palette
con datos fijos. Sin editor editable, buffer/undo alternativo, red, dependencia o
backend elegido. Su contrato distingue Workbench existente de Presentation y
dispatcher aún privados de TUI. Check-scene.py y node --check aprobados por worker
e integrador. [Escena/contrato](M2B_SCENE_STUDY.md).

Tras terminar benchmarks se intentó captura con Edge ya instalado, headless y
perfil propio en work: terminó con 0xC0000005 sin imagen. No se desactivó seguridad,
instaló herramientas ni tocó configuración/servicios. Revisión visual renderizada,
DPI/IME/accesibilidad y todo rendimiento GUI siguen pendientes. No hay jc-gui
funcional ni cierre M2B. Git con escritura/CI sigue en M5; sin push ni cambios a
GPU/Qwen. El foco siguiente de aceptación sigue siendo metadatos densos y puerta
de RAM/latencia física; estos frentes no eliminan esos pendientes.


## M2 — Índice compacto, recolección y disponibilidad de menús — 2026-11-02

Sobre `4f14da9`: un índice compacto sustituye los vectores paralelos de inicio/EOL.
Offsets de 30 bits + dos de tipo EOL; promoción a usize cuando se necesita, sin
truncar. Mantiene rango byte/encoding/revisión y actualizaciones directas/inversas.
Se elimina también la reconstrucción de arenas completamente alcanzables durante
recolección; las arenas con huecos siguen relocándose conservando todo undo/redo.

Suite completa local: **209 pruebas, 0 fallos**, fmt --check, Clippy workspace/
all-targets -D warnings y release. Dos regresiones de ancho/EOL y una de arena
original completa + Added con rama descartada, revisiones/mapas/saved/undo/redo.
Clippy pidió un alias de tipo en collect: se corrigió el alias y repitió el
control fallido, sin repetir suites inalteradas. La promoción >1 GiB se prueba
con metadatos sintéticos; no se afirma apertura real de ese tamaño.

El worker de teclado entregó seis regresiones event-level (campos Replace,
F6/Esc/overlays, disponibilidad por tecla/palette/menú, cierre de vista/última,
alias de tabs/panel y Release/Repeat). Encontró que Enter de menú no verificaba
disponibilidad. Se corrigió en dispatch TUI antes de cualquier efecto: Database
deshabilitado no se abre. Se reconciliaron Ctrl+G/selección ya implementados y
regex deshabilitado en el contrato TUI. Matriz manual/física sigue pendiente.

Comparación serial con ejecutables anteriores conservados e identificados por
SHA-256, mismo ejemplo/corpus/perfil: ocho documentos de 4 MiB/líneas de ocho bytes
pasaron de 73.379.840 a 51.712.000 bytes privados tras frío (~70,0 → 49,3 MiB).
Capacidad de proyección activa 4.718.793 → 2.097.348 bytes. Peak commit de vida
del modelo 81.248.256 → 52.817.920 bytes. El GC de ocho dejó de solicitar otros
33.555.648 bytes y pidió 1024; la línea de 16 MiB dejó de pedir otros ~16 MiB.
La primera pasada sin la optimización de GC también se conserva, no se sustituye
por los datos finales. Frío único 90,46 → 104,13 ms; Right+viewport p50/p95
52,0/56,9 → 52,3/87,2 µs. No se afirma mejora general de throughput.

ConPTY densa before/after: ocho sesiones por lado, una warm + siete retenidas,
dos vistas y visita/cierre de los ocho documentos. Peak commit máximo del editor
75.382.784 → 54.226.944 bytes (71,9 → 51,7 MiB), todavía mayor que 50 MiB.
Suma private contemporánea editor+host+ConPTY 77.672.448 → 56.598.528 bytes;
controlador C# separado. Arranque p50/p95 103,30/114,27 → 107,37/114,43 ms;
Right hasta VT 15,529/16,129 → 15,539/16,086 ms, no pantalla física.
Los labels retained de estos CSV incluyen cierre progresivo: no se usan sus
percentiles como RAM estable. El runner separa closing en futuras corridas.

Los benchmarks terminaron antes de integrar el gate de menú. Se reconfirmaron
los dos recorridos funcionales finales de documento/proyecto, Unicode sin otra
tecla, Go to, preview/reemplazo y undo. Método/hashes/datos y tradeoffs en
[M2_DENSE_LINE_METADATA.md](M2_DENSE_LINE_METADATA.md). Sin repetir benchmarks
inalterados por un cambio de disponibilidad de comandos.

El worker GUI no logró superficie controlable: inventario sin Edge targeteable,
lanzamiento sin devolución/PID/ventana/captura, interrumpido; kernel reseteado,
sin llamadas propias pendientes ni servidor. No se cerró navegador no atribuible
ni se ajustó seguridad/sistema. No se repiten intentos equivalentes. El estado
de lanzamiento desconocido se conserva como limitación de aislamiento global.

Siguen abiertos RAM/picos globales, sufijos O(L), huecos grandes, frío real,
matriz de teclado física, pantalla física, IME/DPI/Linux/GUI/AVM. Sin CI remoto,
push, instalaciones ni cambios de seguridad/configuración/servicios/Qwen/GPU.


## M2 — Proveedor de objetos Git de solo lectura — 2026-11-02

Después de `5890ad0`, el módulo git_read Windows obtiene blobs HEAD/index por
ruta relativa validada/literal. Bytes privados inmutables, identidad de repositorio,
commit/OID y modo, sin normalizar fuente/EOL ni aplicar filtros/textconv. No hace
escrituras Git de producción, config persistente, red/fetch ni indexer/watcher.
El ejecutable instalado es elegido por ruta absoluta; fuera de Windows se rechaza.

Win32 Job Object y handle list limitan herencia y contienen proceso/descendientes
desde creación. Salidas capadas, timeout de solicitud y cancelación cooperativa.
Una auditoría previa encontró retornos sin reap al fallar el primer/segundo reader:
se corrigieron con CaptureGuard RAII, stop/CancelSynchronousIo y observación de
terminación antes de join, incluido unwind del callback. Drenaje completo en éxito;
no se abandona un hilo para cumplir una gracia ni se promete retorno duro de 5 s.

Windows 11 Pro 10.0.26300, Rust 1.85.1 MSVC, Git **2.54.0.windows.1**, offline:
nueve tests dirigidos aprobados y suite completa serial **218 pruebas, 0 fallos**.
Dos tests ignorados son helpers propios invocados por los de contención, no casos
de producto saltados. Verificación OS de hijo/descendiente terminados tras cancel,
timeout, fallo de cada reader y pánico; blob grande completo, stdout/stderr limit,
quoting real, rutas/missing/unmerged/modos y HEAD/index separados. Solo repositorios
sintéticos exclusivos en work/test-temp reciben init/objetos/index/refs de prueba;
index/config/worktree se comprueban intactos durante lectura del proveedor.

Luego se amplió el caso existente HEAD/index para prestar sus bytes a DiffModel:
un hunk exacto con BOM/invalid/EOL conserva ambos snapshots; dirigido aprobado.
Clippy pidió retirar drop(control) innecesario de un test; corregido sin cambiar
producción. Controles finales fmt --check, Clippy workspace/all-targets -D warnings
y release aprobados, sin repetir suites inalteradas por esos cambios de test.

No existe aún UI/status Git ni entrega versionada en JobSystem/TaskStamp. No se
han medido RAM/latencia de Git activo y readers; datos de servicios apagados del
bloque RAM no validan esa carga. Windows 10 no se probó localmente, Linux/IME/GUI/
pantalla física y picos globales siguen abiertos. Contrato/citas primarias en
[GIT_READ_ONLY_PROVIDER.md](GIT_READ_ONLY_PROVIDER.md). Sin push/CI remoto,
instalaciones ni cambios a seguridad/configuración/servicios/Qwen/GPU.
# Integración Git diff versionada y TUI — 2026-11-02

Bloque completo construido antes de validar: ampliación del JobSystem F3,
OwnedDiffModel con fuentes compartidas, controlador de una intención en vuelo,
comandos centrales, configuración de Git por sesión y visor TUI navegable.
No se incorporó otro pool, dependencia ni motor editable. Contrato en
[GIT_DIFF_DELIVERY.md](GIT_DIFF_DELIVERY.md) y
[GIT_DIFF_PRESENTATION.md](GIT_DIFF_PRESENTATION.md).

Validación local Windows/Rust 1.85.1 MSVC/Git 2.54.0.windows.1, offline y serial:

- Suite workspace: 237 pruebas aprobadas al cierre; dos helpers ignorados se
  ejecutan desde sus tests padres. La primera corrida aprobó aplicación/core/texto/
  archivos y detectó en TUI una expectativa de última opción del menú anterior.
  Tras actualizar esa expectativa y corregir el presupuesto de filas del visor,
  se repitió solo `cargo test -p jc-tui`: 40 + 2 aprobadas. La revisión posterior
  separó el presupuesto de hunks laterales del panel inferior; se repitieron TUI,
  Clippy/release TUI y el smoke afectado. Otra revisión corrigió presión temporal
  de snapshots: nueve Git + ocho F3 jobs, Clippy de aplicación y release workspace
  repetidos de forma dirigida. Sin repetir las suites de los demás crates.
- Nueve regresiones de controlador: dirty HEAD/index, bytes inválidos/BOM/legacy,
  cuotas antes de captura, espera bajo presión compartida sin copia extra,
  reserva retenida hasta poll, supersession/cancel/clear/
  retry, obsolescencia por revisión/encoding/vista/documento/ruta/cierre, EOF y
  navegación sin transacción/guardado. OwnedDiffModel conserva fuentes sin copiar.
- Cuatro pruebas de compositor y tres de preparación: clipping, inmutabilidad,
  EOL, prefijos acotados/contexto, scroll largo/End→Up y panel de una fila.
  Dos pruebas TUI conectan disponibilidad/configuración, entrega sin otra entrada,
  recuperación de error y composición sin alterar bytes/revisión/undo.
- `cargo fmt --all -- --check`, Clippy workspace/all-targets con `-D warnings`
  y build release workspace aprobados. Clippy corrigió una construcción de string
  exclusivamente en el fixture de prueba; no requirió repetir suites sin cambios.
- ConPTY release: [datos crudos](benchmarks/M2-git-conpty.json). HEAD e index llegan
  sin otra tecla, navegación a hunk, undo→Stale, hunks laterales conservados al
  ocultar/reabrir el panel inferior, bytes de disco/index iguales y
  salida 0. El fixture usa repositorio exclusivo bajo work, sin tocar refs/index
  del proyecto. Primer intento del harness corrigió lanzamiento de stdin/argumentos;
  los siguientes corrigieron su supuesto de agrupar varios caracteres en un undo.
  En entrada WinAPI ConPTY se usó finalmente un carácter, una edición inequívoca.

Los 31 movimientos del harness son anteriores a iniciar Git: p95 inyección-a-VT
15,9808 ms, cero clears por movimiento y un clear por resize. Su muestreo de memoria
también precede a Git; **no mide memoria/latencia conjunta de auxiliares**, ni es
una aceptación global o de pantalla física. No se repitieron benchmarks generales.
Sin CI/push/instalaciones ni nuevos intentos GUI. Estado completo de repositorio,
watcher, Git con escritura, backend GUI y M2A global continúan pendientes.

## Bloque de sufijos, crecimiento acotado y auxiliares — 2026-11-02

Construcción completa antes de la ventana de pruebas: ejemplo opt-in de Workbench,
política de reserva de replace_after y tres regresiones Compact/Wide/promoción;
instrumentación C# de procesos y runner/resumen de nueve sesiones Git. Sin
dependencias nuevas, cambios de sistema, CI ni intento GUI.

- `cargo test --workspace --locked --offline`: **240 aprobadas, 0 fallidas**;
  dos helpers ignorados se invocan desde sus padres. Se conservan bytes/encoding/
  EOL/mapas y undo en la suite completa tras el cambio productivo.
- fmt, Clippy workspace/all-targets `-D warnings`, build release workspace y
  ejemplo suffix_cost aprobados. csc compiló el harness completo; la etiqueta de
  alcance de conhost se aclaró después de capturar, sin alterar sus contadores.
- [Sufijos](M2_SUFFIX_COST_REVIEW.md): dos corridas quietas de 4914 filas cada una,
  153 resúmenes calientes por corrida, mismo ejemplo/hash. La primera newline de
  8 MiB ya no duplica capacidad lógica: posterior 8388792→4227256 B. Máximos de
  scan/decode por etapa de edición 20/63 B. El sufijo sigue O(L).
- El tiempo frío único de esa operación subió **1760,8→2145,2 µs**. P95 caliente
  apply 601,8→574,2 µs y undo 1135,8→1131,4 µs. No se acepta aceleración general
  ni RAM nativa por capacidad lógica; se amplía distribución fría en otro bloque.
- [Git auxiliar](GIT_AUXILIARY_MEASUREMENT.md): nueve sesiones con release
  `6cf6f13`, antes de cambiar crecimiento. 446 filas / 335 ciclos, Error vacío y
  Truncated false; HEAD/index asíncronos, navegar, stale/undo, sidebar, disco/index
  y salida limpia aprobados. Máximo privado observado editor + descendientes:
  4,594/8,434/11,156 MiB para 4 KiB/1 MiB/(2 MiB−64 B), respectivamente. Incluye
  conhost descendientes Git y excluye bootstrap/conhost inicial/harness. Cadencia
  real p95 47,8086 ms y máximo 64,1638 ms: no pico real ni fotografía atómica.

La captura Git y el perfil de sufijos se ejecutaron en ventanas separadas, sin
builds/fixtures simultáneos. Los datos/hashes están enlazados desde ambos métodos.
No se repitieron benchmarks generales ya aprobados. Quedan activos la distribución
fría y el nuevo recorrido nativo dense-edits; su código todavía no es evidencia.
La memoria densa nativa previa de 51,7 MiB y las puertas global/física siguen abiertas.

## Cierre frío y edición densa nativa — 2026-11-02

Se recogieron los dos entregables, revisaron y ejecutaron en ventanas seriales,
sin compilar ni correr otros fixtures durante la captura. No hubo cambios
productivos; se conserva el checkpoint de 240 pruebas, sin repetirlo por añadir
instrumentación. Formato, Clippy del ejemplo, release y csc aprobados.

- [Distribución fría](M2_COLD_GROWTH_DISTRIBUTION.md): mismo ejemplo/hash sobre
  anterior `6cf6f13` exportado bajo work y actual `d6de969`. Tres pares con orden
  alternado; 5580 filas/180 resúmenes, 1116 sesiones retenidas. Para 8 MiB/inicio,
  p95 anteriores 1424,0 / 3688,9 / 3858,1 µs; actuales 2361,9 / 3399,4 / 3251,5 µs.
  Signo variable, sin atribución causal de sistema ni aceptación de latencia global.
- [Recorrido nativo](M2_DENSE_EDIT_EVIDENCE.md): mismo RunAcceptance, ocho fuentes
  de 4 MiB, newline/undo por teclado. Siete procesos retenidos por versión, todos
  con 8 ediciones/8 undos/8 cierres, SHA-256 de disco iguales y salida 0. Anterior:
  641 muestras de memoria; actual: 638. Máximo histórico de compromiso del editor
  68,160→52,461 MiB. Máximo observado privado tras editar 65,922→50,434 MiB.
  Tras undo la capacidad queda retenida; cerrar baja consumo pero no certifica la
  carga viva. Bootstrap/conhost/harness separados. La puerta de 50 MiB sigue abierta.
- Actual, inyección-a-VT: p95 movimiento 16,1111 ms, inserción 16,0820 ms, undo
  16,5021 ms; son datos del harness con correlación, no pantalla física.
- `summarize-growth-evidence.py` validó hashes/casos/filas y checks antes de generar
  los resúmenes de modelo, memoria por proceso/fase y tiempos VT por separado.

No se reinterpreta el first_cycle único anterior como p95 ni se afirma disco/CPU
fríos: las cachés no se purgaron. Datos, versiones, hashes y limitaciones están
en ambos métodos. El siguiente índice por bloques está solo en construcción
experimental, fuera de la aplicación; todavía no es implementación ni aceptación.


## Índice por bloques productivo y comparación de undo — 2026-11-02

Se sustituyó la ruta plana después de la comparación `191bd82`, sin conservar
dos índices/editing models. Bases usize, relativos/EOL u16, fronteras locales,
catálogo acotado y liberación tras borrado masivo. Oráculos de miles de líneas,
inversas, offsets cercanos a usize::MAX, 512 roundtrips y aplicación real UTF-8
inválido/BOM/EOL mixtos/Windows-1252 explícito/vistas/rangos/disk-preserving.
La página legacy real del usuario continúa sin confirmar.

- Suite workspace locked/offline tras reemplazo: **245 aprobadas, 0 fallidas**;
  dos helpers ignorados se invocan por padres. Tras corregir retención del
  catálogo se ejecutaron las **15 pruebas afectadas de workbench y tres de
  aplicación**, incluyendo una regresión nueva: **246 distintas aprobadas**,
  no una segunda suite completa de 246. fmt, Clippy workspace/all-targets
  `-D warnings` y release workspace bins/ejemplos pasaron.
- [Workbench real](M2_BLOCK_LINE_INDEX.md): dos pares invertidos plano/bloques,
  mismo ejemplo/hash y binarios identificados, por pareja 930 cold + 4914 suffix
  filas por versión. A 8 MiB/inicio p95 first_apply 2373,1/2989,2→60,1/54,2 µs;
  newline caliente undo 1144,4/1147,1→25,0/23,4 µs. Movimiento 2,9/2,7→3,4/3,3 µs,
  ligeramente más lento. Construcción fría similar en ese Workbench; benchmark
  puro histórico y líneas espaciadas conservan sus costes negativos documentados.
- 32 sesiones nativas / 28 retenidas iniciales: base, ocho documentos de 4 MiB,
  línea 16 MiB y denso editado/undo. Checks/cierres/hashes correctos. Pico histórico
  editor 1,414 / 34,906 / 17,957 / **43,727 MiB** respectivamente. Denso previo
  52,461 MiB. Host/conhost/harness separados; logical capacity no se llama RAM.
- [Pipeline undo](M2_UNDO_PIPELINE_PROBE.md): mismo ejemplo sobre D6 archivado y
  bloques, dos pares invertidos, 1984 filas, 496/captura, checks completos y sin
  full repaint. Compilación release y Clippy del ejemplo pasaron. Undo total
  in-process p95 573,7/752,9→179,2/172,1 µs. Son relojes wall time, no CPUcycles;
  el probe tiene calentamiento adicional fuera de timer, documentado.
- Repetición serial nativa: 16 sesiones / 14 retenidas más, todos con ocho
  edits/undos/cierres, disco intacto y salida cero. Undo VT p95 56 muestras:
  plano **16,9702 ms**, bloques **16,9147 ms**; la diferencia inicial de bloques
  20,9661 ms no persistió. No se borra esa captura ni se atribuye causa al núcleo
  o SO. Pico repetido 52,313→43,820 MiB. Total bloque: 48 sesiones / 42 retenidas.
- `summarize-growth-evidence.py --include-block` validó checks nativos y
  `summarize-undo-pipeline.py` identidades/cobertura/etapas/VT, separados. Raw,
  metadata y hashes enlazados desde ambos métodos; comparador histórico en Git.

El corpus denso local queda bajo 50 MiB; servicios/cargas/máquinas y presentación
física siguen sin aceptación global. VT no es pantalla, no se restan percentiles
de protocolos diferentes. Sin CI remoto, push, dependencias, cambios de sistema
ni nuevos intentos GUI. Documentación no motivó repetir suites aprobadas.


## Estado Git por repositorio — cierre local

Proveedor/parser, transporte compartido, controlador y ejemplo público integrados.
Una ventana tras construir todo: **261 pruebas workspace aprobadas, 0 fallidas**,
dos helpers de subproceso invocados por padres; formato, Clippy workspace/all-targets
con warnings denegados y release workspace bins/ejemplos pasaron. Son 15 pruebas
nuevas: seis parser, seis controlador y tres Git real. No hubo CI ni mediciones
de rendimiento adicionales. Los apartados de entrega del worker son históricos.

El fixture real comprobó MM/no rastreado por captura raw, proveedor tipado y
controlador real con Wakeup/poll/join. Index/config/worktree conservaron sus bytes,
sin index.lock. Rechazo de drivers clean/process sin ejecutar centinelas, raíz
padre/cancelación/salida excesiva. El controlador rechaza longitud Y capacidad de
PathBuf excesivas; una ruta corta con reserva enorme no elude la cuota de metadata.
Pruebas de cola/generaciones/configuración/panic/join usan coordinación explícita.

El ejemplo `cargo run -p jc-app --example git_status --release --locked --offline --
ABSOLUTE_GIT_EXECUTABLE ABSOLUTE_REPOSITORY_ROOT` usa la API pública; compila en
release y escapa cada byte de ruta sin conversión lossy ni apertura implícita.
No se ejecutó como otro benchmark: el recorrido real está en el fixture integrado.

La consulta representa configuración aislada y disco/index, no buffers sin guardar
ni necesariamente preferencias globales del Git personal. Filtros configurados,
renames detectados y cambios internos de submódulos quedan fuera de esta consulta.
Rutas byte no autorizan navegación filesystem. TUI status sigue pendiente;
diff activo HEAD/index ya conectado. Sin Git writes/watcher/GUI ni aceptación
global de RAM/pantalla física.


## CPU por archivos y arranque configurable — 2026-11-02

Construcción completa antes de la validación del executor: un scanner común,
coordinador y helpers persistentes, ondas/entrega ordenadas, cuotas de fuente y
resultados centrales, read_file sin duplicar Vec por guardia y cancelación/RAII.
Tres pruebas internas y cinco de API añadidas. La corrida workspace aprobó 122
casos de aplicación antes de encontrar dos expectativas nuevas incorrectas de
case-insensitive en S1. Corregidos SOLO oráculos: 32 casos nuevos/pendientes de
aplicación y 115 de otros crates pasaron, total **269 distintos**. No se repitió
la parte aprobada. Clippy pidió manual_clamp equivalente, resuelto y aprobado;
release workspace bins/ejemplos pasó. Frontera exacta con archivo vacío se verificó
en el caso dirigido después, sin cambiar producto ni repetir la suite.

Después se construyó/validó `--search-workers 1..4`: **47 pruebas TUI** (42 lib,
cinco main), cinco nuevas respecto a 269. Total actual **274 pruebas distintas
aprobadas**, no una corrida completa única de 274. Rechazo antes de TerminalSession,
default2/opciones/rutas/encoding/help/errores y propagación a cada draft comprobados.
Clippy dirigido TUI/all-targets y test afectado de app, release TUI y CLI real
help/error/duplicado pasaron. Formato aplicado; sin inferir validación nueva de
las 269 anteriores ni repetir mediciones tras el flag de frontend.

[Método y resultados completos](CPU_PERFORMANCE.md): hardware 8 físicos/16 lógicos
observados sin alterar afinidad, perfil inicial126 muestras de fase y comparación
1488 consultas conservadas (dos pares, 31/caso) más selección codegen168 muestras.
Fuentes/probe/binarios/corpus identificados por hash, raw/checks/metadata retenidos;
capturas serializadas sin builds/fixtures pesados concurrentes.

- 16 MiB: mediana 1,24–1,49× y p95 1,41–1,48× más rápidos; throughput elegible
  ~67,9–86,4→99,5–108,4 MiB/s. CPU agregada ~1→1,726–1,797 núcleos equivalentes.
- CPU total sumada por operación/caso sube **30,4–47,9%**; se acepta intercambio
  local por menor espera, no menos trabajo ni 100% CPU como objetivo. Un caso
  pequeño mostró mediana ~4,2% más lenta; se conserva en datos/documentación.
- Privado muestreado del harness16MiB 5,527–5,625→3,520–4,230 MiB; reader guardia
  y allocator también cambian costes. No RAM de TUI ni garantía global de 50MiB.
- Overrides solo jc-app s/2/3: seis capturas en dos órdenes, siete/caso; no mejora
  UTF-8 consistente que justifique política global y probe +15,6/+21,9% de tamaño.
  Se mantiene **opt-level=s**, Cargo.toml/lock sin cambios. No repetir ese perfil.

Carga Qwen desconocida, no inspeccionada/controlada; cachés, energía, temperatura
y otra carga no controladas. Accounting CPU cuantizado y picos muestreados, no
instrucciones exactas ni pantalla física. Windows local, sin aceptación Linux,
GUI o servicios completos. Workers4 funcional, rendimiento no certificado.
Sin instalaciones, CI/push ni cambios de seguridad/servicios/CPU global/GPU.

## Presentación Git status integrada — 2026-11-02

Completado el siguiente incremento autorizado después de CPU: sidebar de estado
Git, comandos centrales sin documento abierto, configuración con intención de
status/diff, selección, refresh y cancelación. La preparación produce únicamente
filas visibles acotadas; composición de solo lectura, paths byte escapados sin
conversión lossy ni apertura de rutas. Poll acepta entregas con contexto vigente;
la recuperación de terminal también cubre pánico de una generación descartada.
No se inicia trabajo al pintar ni hay watcher. Se preserva el proveedor aislado,
sin renames/submódulos y con rechazo explícito de clean/process configurados.

Construcción e integración completas antes de la ventana de comprobación:
`cargo test --workspace --locked --offline` aprobó **290 pruebas, cero fallos**;
dos helpers ignorados son invocados por sus padres. Formato, Clippy workspace
all-targets con `-D warnings` y release workspace bins/examples aprobaron.
Son 16 regresiones nuevas: registro 1, recuperación obsoleta 1, componente 5,
renderer 4 e integración Git nativa 5. Los fixtures comparan index, config, HEAD,
ref de rama y archivos, además de bytes/revisión/undo/carets del documento compartido.
Screen lógico de 45/140 columnas comprueba clipping e idempotencia; no es una
medición física. No se repitieron perfiles CPU ni suites aprobadas por documentación.

Uso y contrato en [GIT_STATUS_PRESENTATION](GIT_STATUS_PRESENTATION.md). Sin CI,
push ni escritura Git desde la función. Estado refleja disco/índice, no buffers
sucios ni snapshot atómico del filesystem. Rutas siguen opacas y Enter no abre
archivos. Windows local; GUI, aceptación RAM global y pantalla física abiertas.
Este cierre no declara M2A completo ni autoriza ampliar optimización CPU.

## Selección rectangular acotada — 2026-11-02

Integración completa de geometría, vista, comandos/menu/Palette, clipboard interno
tipado y composición antes de validar. [Contrato y límites](M2_RECTANGULAR_SELECTION.md).
Son 20 pruebas nuevas: seis geometría, diez Workbench y cuatro Screen. TAB4,
virtual, UTF-8/1252/ISO-8859-1/IBM850, encoding estricto, rechazos de caracteres
fuera del repertorio, one-step undo/redo, vistas compartidas, cambios externos,
BOM/bytes inválidos fuera del rango y mixed EOL. No escrituras al archivo fuente.

Corrida inicial workspace: 142 aprobadas antes de una nueva aserción fallida;
selection_for_pane todavía exponía un rango lineal para una selección rectangular.
Se eliminó ese cálculo duplicado y se delegó en ViewState. Después, 53 dirigidas
de app (corrección, f1/panes, rectangle y pendientes s0/workbench) y 138 de otros
crates (60 TUI + cinco main TUI incluidos) aprobaron. Hasta ese punto: **309 pruebas
distintas aprobadas**, dos helpers ignorados invocados por padres; no una corrida completa
única de 310. No se repitieron la suite costosa aprobada ni los perfiles previos.
Formato, Clippy workspace all-targets con -D warnings y release workspace
bins/examples aprobaron. No CI/push, nuevo ConPTY ni medición física.

Limitación material: rectángulo de una acción sobre repertorio explícito de una
celda, con tabs y virtual; Unicode general/grafemas/ancho doble siguen pendientes.
Clipboard es el interno de Workbench; no OS clipboard. Edición efectiva termina
la selección; no multicaret persistente. Columnas0…4096/1024filas/64KiB línea,
2MiB fuente y cuotas lógicas separadas de clipboard/staging, no RAM privada.
Render Screen 45/140/resize/split es evidencia lógica, no aceptación global M2.

Revisión final: extender dentro de TAB/virtual puede cambiar intención visual sin
mover cursor/anchor en bytes. Se descarta la navegación documental diferida en esa
acción; una décima prueba Workbench verifica que la entrega de find no reemplace
el rectángulo. Pasaron las diez pruebas de rectangle, Clippy dirigido de app y
release actualizado. Ese caso adicional completa las **310 distintas**; la fase
anterior de 142 + cierre dirigido + otros crates había reunido 309. No se repitió
la suite global por esta corrección localizada.

## Unir/dividir líneas por la transacción existente — 2026-11-02

[Join/Split](M2_LINE_COMMANDS.md) integrados en modelo, comandos, menú, Palette y
teclado. Enter en Editor se resuelve centralmente a Split; Ctrl+Shift+J une líneas,
conservando Ctrl+J del panel. Join solo propone cortes EOL/indentación ASCII y
espacios, sin reconstruir contenidos. Clasificación raw byte→línea por metadata,
1024 fronteras/2MiB de tramo afectado admitidos antes del plan; se conservan los
límites reales de validación/layout frío/normalización F2, no cota global de trabajo.
Split reutiliza insert_newline. Encoding/BOM/newline final, undo único y mapas de
vistas conservados; no escritura de archivo ni habilitación rectangular accidental.

Nueve nuevas: cinco Workbench y cuatro TUI. 74 dirigidas de app aprobaron; TUI
aprobó 63 antes de fallar un fixture nuevo que usaba Symbols deshabilitado. Solo
ese montaje cambió a Explorer vacío/F6; las cuatro nuevas aprobaron dirigidas.
Total de cierre **319 distintas**, sumando nueve al checkpoint310, sin proclamar
una corrida completa319. Formato, Clippy workspace/all-targets -D warnings y
release workspace bins/examples aprobaron. No se repitieron suites aprobadas ni
perfiles por prosa; sin CI/push/ConPTY nuevo. Screen45/140 readonly no acredita
display físico/RAM global/GUI. AVM/proveedores M3 no intervienen.
