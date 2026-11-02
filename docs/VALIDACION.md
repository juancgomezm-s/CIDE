# Validación

**Evidencia local más reciente:** F1/F2/F3, guardado Windows y Search Plus literal
de proyecto, 174 pruebas y verificaciones Rust satisfactorias. Véase S1 al final; las secciones
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
