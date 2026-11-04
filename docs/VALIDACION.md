# Validación

## Estado vigente — recuperación periódica y limpieza segura, 2026-11-03

Recuperación automática en Rust integrada en GUI y TUI: captura cooperativa de
64KiB por turno, cola de un tramo, escritura/verificación en worker y detección de
versiones obsoletas. Programa tras2s sin actividad y fuerza un intento a30s;
un documento pendiente que cambia cancela la captura incompleta. Fallos esperan10s.
Tras la captura inicial, una sesión sin cambios no reescribe sus copias.

La limpieza conserva HEAD/BACKUP verificados, borra únicamente generaciones
obsoletas del slot propio por handles Win32 y rechaza enlaces/archivos desconocidos.
Las escrituras incompletas propias tienen guarda RAII. La limpieza aplazada vuelve
a intentarse sin reconstruir documentos ni avanzar HEAD. El cierre normal espera
el worker y confirma después la última edición. [Contrato](PERIODIC_RECOVERY_RETENTION.md).

20 nuevas pruebas aprobadas por suites/cierres dirigidos, más regresiones de
recuperación y terminación abrupta. Tres procesos reales de GUI y tres de TUI
pasaron7 comprobaciones por interfaz con terminación forzada, fuente original
intacta, dos generaciones protegidas y ausencia de reescrituras en reposo.
Los enlaces duros/junction y Guardar como de fixtures necesitaron ventana local
sin la restricción del sandbox. No se cambiaron garantías para hacerlos pasar.
Fmt, Clippy todos los targets sin advertencias y release offline aprobados;
no corrida runtime global única. Evidencia: `docs/evidence/periodic-recovery-2026-11-03-*`.

Ejecutables actuales: `target/recovery-next/release/cide-gui.exe` y `cide-tui.exe`.
Se conservan los ejecutables anteriores. Depósitos personales intactos en pruebas.
No se modifican archivos originales salvo Guardar explícito. Siguen abiertos la
integración GB, aceptación física/global, IME/DPI/a11y/clipboard real y M3–M5.
Sin dependencias nuevas, CI, push ni publicación.

El siguiente avance es histórico y queda subordinado a este cierre.

## Estado previo — marco GUI, ratón TUI y recuperación Rust, 2026-11-03

GUI ahora tiene marco de título integrado, barra de menús con el mismo fondo
`#2E3440`, pestañas redondeadas por grupo, selector de documentos abiertos y árbol
en píxeles con fuente de interfaz separada del código. Reutiliza una superficie
gráfica y el frame preparado en exposiciones. [Construcción](M2B_GUI_TABS_BROWSER.md).
TUI incorpora ratón y explorador modal de archivos/carpetas, con ayuda en español:
[uso y alcance](M2_TUI_MOUSE.md).

Ambas interfaces recuperan documentos sin guardar, bytes originales, undo/redo,
pestañas y vistas al salir y volver a abrir. El almacén Rust usa generaciones
inmutables, sincronización, verificación por relectura y HEAD/BACKUP atómicos;
si falla la confirmación, el editor permanece abierto. [Contrato y límites](SESSION_RECOVERY.md).
No hay todavía checkpoint periódico ni recolección de generaciones antiguas.

Check de todos los targets, fmt, Clippy sin advertencias y release offline aprobados.
Pruebas dirigidas de recuperación de core/archivos/aplicación/GUI/TUI, terminación
abrupta de procesos y reapertura ConPTY aprobadas; no corrida runtime global única.
GUI:15 checks de shell y13 de interacción, cero errores de pintura; TUI: navegador5,
ratón7 y recuperación4 comprobaciones en tres procesos; GUI también restauró cambios,
caret e historial en tres procesos reales ocultos. Fuentes fixture intactas.
Evidencia: `docs/evidence/gui-shell-browser-2026-11-03-*`.

Ejecutables actuales: `target/gui-next/release/cide-gui.exe` y `cide-tui.exe`.
Los anteriores `target/release` y `target/ui-es/release` se conservan.
La fuente paginada aislada leyó un fixture disperso de5GiB; el editor completo aún
no admite GB. Se eligió instantánea privada y falta integrarla con Document y los
índices progresivos: [auditoría](CORE_GIGABYTES_ADMISSION.md).
Aceptación física/global, IME/DPI/a11y/clipboard real y proveedores M3–M5 abiertos.
Sin nuevas dependencias externas, CI, push ni publicación.

Los estados siguientes son históricos y no sustituyen este avance.

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


**Evidencia local más reciente:** Git read-only con tareas versionadas y visor TUI,
339 pruebas Rust distintas aprobadas al integrar Quick Open y GUI nativo inicial
(cierre selectivo; no una corrida única de339), más16 checks de smoke Win32/GDI,
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

## Quick Open y primer GUI nativo — 2026-11-02

Construcción amplia de M2A+M2B antes de comprobar: [Quick Open](M2_QUICK_OPEN.md)
(metadata finder/selector TUI/abrir existente o lateral/encoding/pin) y [GUI real](M2B_NATIVE_WINDOWS.md)
(HWND/menu propio, modelo único, edición/undo/grupos/guardar, GDI readonly).
Solo jc-gui se añade a Cargo.lock, sin crates descargados ni cambios de versiones
externas. Win32/GDI usa bibliotecas existentes del sistema; no instalación.

La corrida workspace pasó148 casos antes de un fixture symlink no autorizado
por privilegio Windows1314. Se sustituyó únicamente el fixture por junction propia:
pasaron42 dirigidas de app y152 de otros crates (incluyendo70 TUI,2 GUI CLI y2
scoped_open).338 distintas en ese punto; una regresión nueva de caret/selección
sin contaminar navegación, másF1/S0 afectados, pasó18 dirigidas y dejó **339 distintas**.
Dos helpers ignorados se invocan por sus padres. No una corrida única339 ni
aceptación de plataformas no ejecutadas. Clippy pidió colapsar un else-if; corregido.
Formato, Clippy workspace/all-targets -D warnings y release workspace bins/examples
aprobaron; después solo controles GUI/afectados por hallazgos concretos, no suites
globales ni perfiles CPU/índice repetidos.

Smoke release real: HWND propio oculto, **16 checks** completos, cero errores de
WM_PAINT, raster GDI1000x650 con106 colores, guardado real conservando13B exactos.
La revisión inicial del BMP encontró un error no contado; se reforzó el reporte y
el clipping vacío/parcial antes de aceptar este bloque. Se retiró estado espejo
de untitled, se evita Save All sin flujo Save As y se usa caret placement normal
para mouse. Evidencia [JSON](evidence/m2b-native-smoke-2026-11-02.json),
[hashes](evidence/m2b-native-smoke-metadata-2026-11-02.json) y
[canvas PNG nativo](evidence/m2b-native-gui-2026-11-02.png).

No medición de display físico/RAM global/IME/DPI/accesibilidad ni OS clipboard;
el canvas es rasterización propia del renderer, no screenshot del escritorio.
Pruebas manuales de usuario quedan al final, sin solicitarlas ahora. Regex sigue
pendiente, no se descargaron dependencias ni se ejecutó CI/push/AVM. Se continúa
el roadmap en GUI Quick Open/Palette con los contratos ya disponibles.


## M2B: Quick Open/Palette/carpeta nativos — 2026-11-02

Bloque construido completo antes de su ventana serial de validación. Suite
workspace locked/offline: **350 aprobadas**, cero fallos, dos helpers de procesos
ignorados en ejecución directa e invocados por pruebas padres. Nuevas:3 notifier,
4 selector,3 geometría/composición y1 CLI. Formato y Clippy workspace all-targets
-D warnings, release workspace bins/examples aprobados. Se corrigió un nombre
de campo del smoke tras el primer error de compilación; ninguna suite pasada se
repitió por documentación. Sin CI/red/dependencias nuevas ni perfiles CPU/index.

Ejecutable release: **23 checks nativos aprobados**, native_paint_errors=0 y13
bytes de fixture final intactos. Los7 nuevos cubren PostMessage real sin otra
tecla, consulta sin editar, dirty/open-to-side/undo, palette en un undo, comando
no conectado rechazado, raíz invalidada y popup GDI sin mutar revisión. Retenido
en work/native-gui-smoke/20261103T014708Z-0be3b3ba871a; reporte/hash/PNG copiados
en docs/evidence como m2b-native-selectors*. Canvas1000x650/162 colores revisado.
[Contrato, fuentes y evidencia](M2B_NATIVE_SELECTORS.md).

No acredita interacción con el folder picker modal, pantalla física, OS clipboard,
IME, DPI, accesibilidad, RAM global ni otra plataforma. Fallo PostMessage exige
refresh manual si no llega otra señal; no hay transporte OS alternativo. wall_ms
del smoke no es tecla-a-pantalla. Las pruebas manuales de Juan siguen para el final.


## Tres funciones M2: invisibles y buscar/reemplazar bloques — 2026-11-02

Implementadas TUI y GUI, preservando el trabajo de clipboard/encoding. Cierre:
**376 casos Rust distintos aprobados**, por suites119+86+171; no una corrida única.
Dos helpers de procesos ignorados directamente se invocan por sus padres. Formato,
Clippy workspace all-targets -D warnings y release workspace bins/examples pasaron.
Errores de integración corregidos: visibilidad interna de delete_selection y
nombre owner sombreado; fixture EOF corregido para colocar su caret. No se
repitieron las suites119 ya aprobadas al completar las pendientes.

TUI tiene4 nuevas E2E multilínea y4 de invisibles; jc-app suma mapas sparse/ASCII,
campos compartidos,5 handoffs clipboard y3 roundtrips integrados; GUI suma3 pruebas
puras UTF16. Incluyen encoding estricto, BOM/EOL/control/Unicode/EOF/nohit, fuente
intacta al toggle, scroll/caret/selección por spans y un undo de replace-all/save.
Detalles y [logs completos](evidence/m2-visible-blocks-validation-2026-11-02.log)
en [contrato](M2_INVISIBLES_MULTILINE.md).

Release GUI real: **29 checks nativos pasaron**, cero errores de pintura, fixture
final13 bytes intacto. Incluye paste/find cruzando CRLF/replace-all de distinto
número de líneas/undo/save exacto, encoding/cancel sin editar y toggle con draft
intacto. Se revisaron canvas GDI de invisibles y campos; un feedback obsoleto tras
rechazo de cuota se corrigió y validó solo con Clippy/build frontend y smoke29.
Último run: work/native-gui-smoke/20261103T023754Z-ffe68ae83162. Reporte/hash/PNG
retenidos como m2-visible-blocks*, m2-invisibles-native* y m2-multiline-native*.

Clipboard en memoria en el smoke, pasando handlers/modelo de producción; no se
leyó/escribió el OS clipboard. Sus3 pruebas son puras de wire UTF16, no prueba
de interoperabilidad entre aplicaciones. Tampoco se certifican pantalla física,
IME/DPI/accesibilidad, RAM global o plataforma distinta. wall_ms no es latencia.
El diagnóstico escapa todo noASCII para anchura cierta y no acepta un motor de
graphemes/shaping normal. Cache diagnóstico128KiB + textual896KiB=1MiB lógico;
una revisión invalida el cache diagnóstico y la línea fría puede escanearse entera
con buffers acotados. No hubo dependencias nuevas, Actions/push ni nuevos perfiles.


## Save All nativo y targets de guardado — 2026-11-02

Bloque completo construido antes de su ventana de checks.45 pruebas dirigidas
aprobaron:29 jc-app y16 GUI, con6 nuevas (targets y plan puro). Conjunto histórico
**382 distintos**, no una corrida382. Formato/Clippy workspace all-targets/release
workspace bins/examples aprobaron.32 checks nativos del release,0 errores de
pintura y fixture13B intacto. No se repitió la suite376 por este bloque.

Save All ejecuta una intención por DocumentId, nunca por vista; cap1024 y paths
OsStr::len4096. Nuevos requieren path, modal sin préstamo, target revalidado tras
retorno. Cancelación/error reportan éxitos/pending sin rollback global. Save As
normal también captura target antes del diálogo. Tests observan source/EOL/legacy/
BOM, rutas obsoletas por revisión/rename/cierre, vistas/selección/undo intactos y
no escritura del nombre generado. Smoke prueba menú/tecla, cancelación parcial,
Save As nuevo y conflicto externo sin overwrite con decisión inyectada propia.
[Contrato/evidencia](M2B_SAVE_ALL.md). Diálogos físicos/OS clipboard/IME/DPI siguen
sin certificar; no solicitudes manuales ahora, dependencias/CI/push/perfiles nuevos.

## CIDE, iconos y Search Plus GUI — 2026-11-02

Bloque amplio construido antes de validar.391 casos distintos aprobados por
suites:260 completos en el primer recorrido y131 de los paquetes restantes tras
corregir geometría de cliente diminuto; dos benchmarks ignorados. No se declara
una corrida391 ininterrumpida. Nueve regresiones nuevas observables: apertura
versionada3, lista3, geometría2 y parser1. fmt, Clippy workspace all-targets y build
release workspace bins/examples aprobaron; tras corregir WM_NCCREATE solo GUI
se recompiló/revisó. Doc-tests restantes completaron sin casos adicionales.

41 checks del ejecutable canónico en HWND oculto propio:1 pintura nativa,0 errores,
fixture13B intacto. S1 usa snapshot dirty/multilínea, notificación sin otra tecla,
navegación sin reload/undo, stale/refresh, cancelación sin resultados tardíos,
conflicto de disco y root invalidation. Se corrigieron errores detectados, incluidos
el título y la revalidación de disco al reutilizar un documento ya abierto. Primer
smoke con filesystem sandbox falló; release idéntico pasó fuera del sandbox.

Se ejecutaron seis ayudas, pares canónico/alias idénticos y exit0. Se inspeccionaron
recursos PE en seis ejecutables: siete frames16..256 exactos a los ICO fuente,
verde TUI/CLI y naranja GUI. Ventana verifica título CIDE y carga de grupo101.
Cargo.lock y dependencias no cambiaron. ZIP de desarrollo local, sin instalador ni
publicación; notices offline con limitaciones explícitas.

[Contrato y logs](M2B_PROJECT_SEARCH_NATIVE.md),
[metadata/hashes](evidence/cide-project-native-2026-11-02-metadata.json),
[reporte41](evidence/cide-project-native-2026-11-02-native-report.json),
[marca/recursos](CIDE_REBRAND.md). PNG retenidos son conversiones sin pérdida de
bitmaps GDI, no capturas de pantalla física. IME, clipboard OS, DPI/accesibilidad,
targets globales RAM/pantalla y demás puertas siguen abiertos. Sin CI/push,
instalaciones ni nuevos perfiles. Las pruebas manuales del propietario son al final.

## Reemplazo de proyecto y Git gráficos — 2026-11-03

53 Rust distintos dirigidos aprobados:30 GUI,10 S1,9 GitDiff y4 TUI de proyecto.
Los25 GUI intermedios se solapan; no se suman dos veces. Ocho pruebas nuevas,
histórico399 sin corrida399. fmt, Clippy workspace/all-targets y release workspace
bins/examples aprobaron; la observación Clippy de format_collect en fixture se
resolvió con write!, sin repetir suites ya aprobadas. No nuevos perfiles/CI.

58 checks nativos del release final:17 nuevos (10 reemplazo/7 Git),0 errores de
pintura, fixture13B intacto. Vacío elimina, no-op conserva revisión/redo y metadata
del cerrado; preview/Enter no confirman, F9 duplicado no aplica dos veces, un archivo
por TASKS_MESSAGE permite cancelar los pendientes. Abiertos conservan one-undo y
no se guardan; conflicto de disco/encoding/root no sobrescribe destinos.

Git status no incorpora Q dirty; HEAD e index identifican blobs distintos y diffs
comparan contra el snapshot dirty exacto. Enter status no navega, Enter hunk revalida
sin editar; undo marca Stale, refresh explícito, cancelación y root/executable
invalidation. Pintura no copia/admite snapshots ni consume completions. Todos los
hashes .git del repositorio sintético permanecieron iguales (index/config/refs/
objetos/logs). Preparación de datos Git escribe solo en el fixture propio, nunca
en el repositorio del proyecto. Seis ayudas/aliases e iconos PE verificados.

Primera corrida nativa falló por un oráculo revisionID+1 tras Undo; se corrigió
la expectativa, manteniendo el Undo exacto como prueba observable, se recompiló
solo GUI y el recorrido integral58 pasó. Se retienen ese fallo y logs crudos.
La revisión GDI corrigió además el draft de varias filas y el estado Git visible;
solo GUI se recompiló y su recorrido58 pasó de nuevo. Artefactos final-* identifican
las fuentes/binarios de ese cierre, sin sumar ambos recorridos como casos nuevos.
[Contrato/evidencia/hash](M2B_PROJECT_REPLACE_GIT.md). PNG de GDI sin pérdida;
no acredita pantalla física, clipboard OS, IME/DPI/accesibilidad o RAM global.
Esas pruebas de Juan se conservan al final; no instalaciones, dependencias nuevas,
Git con escritura productiva, CI remoto ni push. Próximo contrato Explorer M2B activo.

## Explorer compartido TUI/GUI — 2026-11-03

Construcción integral antes de probar: catálogo/proveedor/vista de jc-app,
enumeración por handle jc-files, retirada de ruta TUI previa, sidebar/foco/mouse/
teclado GUI y arneses. **352 casos aprobados**, cero fallos y dos helpers ignorados,
34 suites de jc-app/jc-files/jc-tui/jc-gui; no corrida de todo el workspace. Trece
casos nuevos y retiro de un test de la ruta eliminada. Incluye cuotas, Busy,
cancel/panic/root obsoleto, nombre UTF-16 no escalar, junction real y rename real
de ancestro impedido durante enumeración; dirty reuse con hoja cambiada rechazada,
split/pin/undo y frames repetidos sin admisión. No se suman corridas solapadas.

fmt, Clippy workspace/all-targets con -D warnings y release bins/examples aprobaron.
Clippy inicial señaló import obsoleto, if colapsable y alias de tipo del fixture;
corregidos sin repetir las suites aprobadas. Logs iniciales y finales conservados.

**70 checks nativos aprobados**:58 anteriores y doce nuevos Explorer. HWND oculto
propio, seis WM_PAINT, cero errores; metadatos Git sintéticos y fuente13B intactos.
Entrega sin entrada, expansión/disclosure, Tab/texto/paste sin editar, menú/palette
desde contexto Explorer, apertura normal/lateral con dirty/pin/undo, GDI de solo
lectura, cliente compacto y rechazo de targets obsoletos/cancelados/otra raíz.
Primer intento se detuvo por privilegio1314 del symlink opcional: ahora se omite
explícitamente; no se cuenta. El test separado de junction real sí se ejecutó.

**Seis flujos de terminal aprobados**, salida cero,37 736 bytes VT y fuentes exactos:
ConPTY propia, entrega sin otra tecla, árbol lazy, apertura, refresh/cancel, dirty
reuse con un undo y resize80×24→120×32. Dos primeros intentos fallaron por esperas
del arnés que confundían título/pestaña con menú/fila. Las esperas finales identifican
bordes de menú y filas del árbol. Se conservan reportes fallidos y final; no son
muestras de rendimiento ni se cuentan como nuevos casos. Sin .NET ni dependencias.

Seis ayudas/aliases y siete frames de icono por cada PE aprobados. PNG Explorer GDI
1584×841 convertido sin pérdida y revisado: sidebar reserva área, árbol expandido,
dirty activo y segundo grupo visibles. [Contrato/evidencia](M2B_SHARED_EXPLORER.md),
[hashes](evidence/m2b-explorer-2026-11-03-metadata.json). No acredita Linux/fallback,
pantalla física, clipboard OS, IME/DPI/accesibilidad ni RAM global; M2A/M2B abiertos.
No CI/push/instalaciones, perfiles cerrados repetidos ni pruebas manuales solicitadas.

## Navegación nativa compartida — 2026-11-03

Construcción/cableado completos antes de validar. **78 casos Rust distintos**
aprobados:44 S0/F1/panes/workbench,30 GUI y cuatro nuevos TUI. Primera corrida TUI:
tres aprobados y uno falló al suponer que Go to desde grupo fijado permanece allí.
PaneManager dirige navegación automática al otro grupo; corregido el oráculo en
ambos arneses, solo ese caso TUI se repitió y pasó. No se alteró PaneManager ni se
cuentan dos veces intentos solapados. fmt, Clippy workspace/all-targets -D warnings
y release workspace bins/examples aprobaron. No corrida global del workspace.

**14 checks nativos aprobados**, siete nuevos, sección navigation en HWND propio
oculto: arranque4, navegación7, bitmap/pintura3. Un WM_PAINT, cero errores; invocación
por menú/paleta/atajos, coordenadas BOM/Unicode/CRLF/invisibles, límites/gramática/
fuera de fuente/cancel/no preview, dos historias con pin/split y destinos obsoletos
por revisión/encoding/vista/pane/cierre. Historial multilínea+flags conserva reemplazo
y campo activo incluso por palette/cancel. F3 diferido no navega tras Go to pero
scan termina Complete; fuentes/undo intactos. Caso disabled no cierra el modal ni
edita su cursor. Fuente13B y guard de metadataGit propios intactos, sin revalidar
proveedores Git ni ejecutar recorridos Explorer cerrados. No full smoke global.

Seis ayudas/aliases e iconos siete frames por PE aprobados. Dos PNG1000×650 GDI→PNG
sin pérdida retenidos, panel y restauración de fuente revisados. Logs crudos, fallo
inicial y fuentes/binarios SHA-256 en [contrato](M2B_NATIVE_NAVIGATION.md) y
[metadata](evidence/m2b-navigation-2026-11-03-metadata.json). No perfiles repetidos,
dependencias, instalaciones, CI o push; sin clipboard OS ni manual intermedio.
M2A/M2B, IME/DPI/accesibilidad, Linux y aceptación RAM/pantalla continúan abiertos.

## Líneas y comentarios nativos — 2026-11-03

Bloque completo construido/cableado antes de checks: seis comandos, registro,
foco/menús/atajos/paleta y recorridos. **23 casos dirigidos aprobados a la primera**:
15 interacción, tres TUI nuevos y cinco parser CLI GUI; no corrida global. fmt,
Clippy workspace/all-targets -D warnings y release bins/examples aprobados.

**14 checks nativos aprobados**: siete nuevos y siete arranque/pintura, sección
line-edit exclusiva en HWND propio oculto. Un WM_PAINT, cero errores. Selección/
caretline, Delete/Move y EOF vacío/sin separador, BOM/EOL mixtos, ABL/C#/Python/plain,
cuatro encodings, capacidades ausentes deshabilitadas, UN undo/redo exacto y split/
pin con selección inactiva rebasada. Menú/teclas/paleta reales; modales/Explorer y
paleta obsoleta no editan fuente oculta. Documento limpio→duplicate dirty→undo limpio
preserva DocumentId/ViewId y disco13B sin Save. No se asumen IDs de revisión +1
tras ramas undo/redo; la reversión exacta es la evidencia de un undo lógico.

Seis ayudas/aliases, iconos siete frames/PE y guard de metadataGit propia aprobados;
este último no es regresión de proveedores Git. Dos PNG1000×650 GDI→PNG sin pérdida
revisados. [Evidencia](M2B_NATIVE_LINE_EDIT.md),
[hashes](evidence/m2b-line-edit-2026-11-03-metadata.json). No suites de navegación/
Explorer ni perfiles cerrados repetidos. Sin CI/push/dependencias/instalaciones,
AVM o clipboard OS. Manual/global, Linux, symlink omitido y notices GNU pendientes
siguen separados; M2A/M2B no se cierran por este bloque funcional.

## Selección literal y reapertura GUI — 2026-11-03

Construcción integral antes de checks; **11 casos dirigidos** aprobados:dos core,
cuatro TUI nuevos y cinco parser CLI. Primera TUI:3 aprobados/1 falló por asumir
scan pendiente con query Complete; corregido setup para exigir Running/selección
original y repetido solo ese caso. Primer compile GUI falló por getter privado
en fixture; usa ahora observación de vistas públicas y cola de decisiones propia.
Se retienen ambos logs. fmt, Clippy workspace/all-targets -D warnings y release
bins/examples aprobados; no corrida global del workspace ni API ampliada para tests.

**15 checks nativos aprobados**, ocho nuevos y siete arranque/pintura, sección
selection-reopen; un WM_PAINT/cero errores. Cuatro encodings y selección multiline,
flags,4096 raw válido/4098raw y8192UTF8expansion rechazados, invalidbytes/empty/rect,
foco y paleta stale, cancel y versión/vista tardías. Saved-file reaparece con disco
actual/TabId retenido/DocumentId nuevo, sin undo; new-file vacío. Cierre compartido
no crea closedentry, último dirty cancel/discard y confirmación obsoleta verificados;
missingfile retry/LIFO y colisión con dirty abierto preservan metadata/datos. Tab1…8/
último mantienen identidad/caret/selección/flags. Sin guardado del fixture original13B.

Dos PNG1000×650 GDI→PNG sin pérdida revisados, fuente y metadataGit propias intactas
(no otra suite de proveedores). Seis ayudas e iconos PE aprobados.
[Contrato/reportes/logs](M2B_SELECTION_REOPEN.md),
[hashes](evidence/m2b-selection-reopen-2026-11-03-metadata.json). No suites cerradas
de líneas/nav/Explorer ni perfiles repetidos, OS clipboard/AVM/CI/push o instalación.
M2A/M2B y gates físicos/globales siguen abiertos; notices GNU/symlink no resueltos.

## Rectángulo GUI y grid compartido — 2026-11-03

Bloque completo antes de checks. **26 casos dirigidos distintos aprobados**:
16 modelo afectado, cuatro TUI nuevos, uno invisible-grid afectado y cinco CLI GUI.
Primera TUI3/4: Find dejaba editor_input=true; corregido contexto, repetido solo
ese flujo. Native inicial encontró flecha Derecha en EOF virtual sin salir del
rectángulo; BOF/EOF corregidos y caso core ampliado/repetido sin contarlo dos veces.
Compiler shadow de owner y lint de precedencia en fixture corregidos. Logs crudos
retienen fallos; fmt/Clippy workspace/all-targets -D warnings y release bins/examples
aprobados. No corrida global ni suites/perfiles cerrados repetidos.

**15 checks HWND propios aprobados**, ocho nuevos: comandos/codificaciones/BOM/EOL,
clipboard interno/altura/EOF, Delete/Backspace/Tab/noop/undo, grid/invisibles/resize,
foco/paleta/intención visual, split/pin/invalidate por revisión/encoding, repertorio/
cuotas y Alt+drag/capture/Up/CancelMode/Escape/stale. Un WM_PAINT, cero errores,
Source13B y metadataGit sintética intactos; no otra suite Git. GDI PNG1000×650
sin pérdida revisados, ayudas/aliases e iconos PE aprobados.
[Contrato/evidencia](M2B_RECTANGLE_GUI.md),
[hashes](evidence/m2b-rectangle-2026-11-03-metadata.json). No mouse físico/clipboard OS,
IME/DPI/accesibilidad/Linux/RAM global certificados. M2A/M2B, symlink y notices GNU
abiertos. Sin dependencias/proveedores/AVM/instalaciones/CI/push/publicación nuevos.

## Inventario GUI y edición/grupos — 2026-11-03

Inspección estática completa de 104 CommandId y rutas reales, con matriz de binding,
menú/paleta, disponibilidad, handler y evidencia histórica. 85 supported/entradas
COMMANDS tras cerrar los huecos de edición/grupos; 19 sin soporte directo por motivo.
No se declara paridad completa ni recorrido físico de los 104 comandos.

Bloque completo antes de pruebas. **Dos casos Rust dirigidos aprobados**: un nuevo
flujo TUI con cuatro encodings/foco/undo y el parser CLI GUI afectado. **12 checks
HWND aprobados**, cinco nuevos: Indent/Outdent por teclado/menú/paleta y undo exacto,
gates modal/Explorer/stale, Join/Split/rectángulo, scroll delta/pin y resize/composición
readonly/limpieza. Un WM_PAINT, cero errores; fixture13B y metadata Git sintética
intactos. No Save/OS clipboard ni suites cerradas. fmt, Clippy workspace/all-targets
-D warnings y release workspace bins/examples aprobados. Primer Clippy falló por
format_collect del fixture; corregido a repeat antes de release y retenido en logs.

[Inventario/contrato](M2B_GUI_INVENTORY.md),
[datos estáticos](evidence/m2b-gui-inventory-2026-11-03.json),
[hashes/logs](evidence/m2b-gui-parity-2026-11-03-metadata.json),
[reporte nativo](evidence/m2b-gui-parity-2026-11-03-native-report.json).
PNG GDI1384×741 y984×641 revisados visualmente, conversión sin pérdida. No corrida
global de pruebas ni perfiles/RAM nuevos, pantalla física/IME/DPI/accesibilidad/
clipboard externo/Linux certificados. Host genérico de paneles GUI y gates previos
siguen abiertos. Sin CI/push/dependencias/proveedores/AVM/instalaciones nuevos.

## Host Problems/Output GUI y entrega F3 — 2026-11-03

Bloque completo antes de checks: almacén/targets/IDs compartidos, JobRecord.path,
aceptación F3 única, TUI, GUI/controladores, menú/paleta/teclado/foco/layout/hits/GDI.
**23 casos Rust distintos aprobados**: cuatro nuevos TaskPanels, ocho jobs y nueve
Git diff afectados por path/dispatcher, uno TUI F3 ampliado con navegación/stale y
uno parser CLI GUI afectado. Sin suite global ni regresiones cerradas sin impacto.
Clippy workspace/all-targets -D warnings, release bins/examples y formato aprobados.

**13 checks HWND aprobados**, seis nuevos y siete startup/pintura. Productores
sintéticos de prueba usan JobSystem/Wakeup/TaskOutput reales y TASKS_MESSAGE propio;
no prueban servicio de negocio. Vacío y focos, edición protegida, typed delivery y
navegación dirty/split/pin, stale revisión/encoding/ruta/raíz/cancel, fallo/cuota/
reinicio, propuesta inerte, recorte/selección/hits/resize/composición y limpieza.
SaveAs usa solo memdoc y path nuevo propio, verifica bytes y elimina ese archivo;
fuente original13B y metadata Git sintética no cambian. Dos WM_PAINT/cero errores.

Corrida inicial aprobada retenida; final añade procedencia visible y bitmap
Problems, con fixture que compara workers existentes para no asumir inicio vacío
al ejecutarse tras otras secciones. Solo sección nativa afectada revalidada y
Clippy/release reconstruidos; Rust aprobados no repetidos. Sin fallos funcionales
en esta ventana. PNG1000×650/1384×741/984×641 GDI→PNG sin pérdida revisados.
[Contrato](M2B_BOTTOM_PANELS.md), [hashes/logs](evidence/m2b-bottom-2026-11-03-metadata.json),
[reporte final](evidence/m2b-bottom-2026-11-03-native-report.json),
[inicial](evidence/m2b-bottom-2026-11-03-native-initial-report.json).
No AVM/proveedor nuevo, OS clipboard/IME/DPI/accesibilidad/pantalla física/Linux,
RAM global o Unicode general certificados; gates previos/notices/symlink siguen
abiertos. Sin dependencias/instalaciones/CI/push/publicación ni perfiles nuevos.

## Entrada y clipboard sintético de selectores GUI — 2026-11-03

Construcción completa antes de checks: API de campo/selección y presentación,
modelos de los tres selectores, entrada/menú/paleta/foco, generación/intención,
decoder acotado y handoff reentrante, GDI y sección nativa dedicada. **23 casos
Rust distintos aprobados**, cuatro nuevos: cinco compartidos de campo, siete
selector/modelo/pintura, cuatro decoder, seis TUI multilínea afectados y un parser
GUI. Los logs tienen24 pases: un decoder apareció también por el filtro selector.
Metadata deduplica nombres; no corrida global ni24 casos distintos.

**13 checks HWND aprobados**, seis nuevos: tres kind con edición/selección/clipboard
por teclado/menú, Unicode fiel y cuota/rechazos atómicos, QuickOpen metadata real y
supersession/dirtyReuse/side/pin, nueve reentradas tardías rechazadas (caret/query/
kind/rev/view/encoding/root/ABA/candidato), foco Find/Explorer/Bottom y encoding,
resize real/compact/composición/limpieza. Transporte únicamente sintético; **no se
abrió, leyó ni modificó clipboard privado**. Source13B, encoding, id/revisión,
catálogo y metadata Git sintética conservados. Un WM_PAINT, cero errores.

Fallos retenidos: expectativa oversize debía contener espacios escapados `\s`
(cuatro casos ya aprobados no repetidos, solo uno corregido), dos compilaciones
del arnés por getter privado sustituido con vistas públicas, y Clippy large_enum
resuelto con Box solo en handoff explícito. Clippy workspace/all-targets -D warnings,
release workspace bins/examples y fmt finales aprobaron. Native a la primera;
sin más suites/perfiles por docs. [Contrato](M2B_SELECTOR_INPUT.md),
[logs/hashes/nombres únicos](evidence/m2b-selector-input-2026-11-03-metadata.json),
[reporte](evidence/m2b-selector-input-2026-11-03-native-report.json).
PNG1384×741 y984×641 GDI→PNG sin pérdida revisados visualmente. No OS clipboard
interoperable/IME/DPI/accesibilidad/mouse-pantalla físicos/Unicode general/Linux/RAM
global aceptados. Menús/draglineal/wheel, symlink opcional y notices GNU pendientes.
Sin dependencias/proveedores/AVM/regex/instalaciones/CI/push/publicación ni seguridad.

## Menús y ratón GUI — 2026-11-03

[Contrato y alcance](M2B_NATIVE_INTERACTION.md). Construcción completa antes de
checks:16 casos Rust distintos aprobados, cuatro nuevos. Viewport/selección byte(1),
panes afectadas(9), menú(2), CLI(1), viewport F2 por rangos(1), presentación/split
TUI afectados(2). Sin solapamientos de filtros ni suite workspace global; solo
rutas impactadas por el cambio de viewport/captura/menú.

13 checks HWND, seis nuevos: cuatro encodings/BOM/EOL mixto, drag/Shift y diagnósticos,
fronteras byte/CRLF y undo; captura/interrupción/resize/seis stales; wheel signed
fraccionaria sobre pane inactivo, foco/caret/selección/pin/enlace y reveal manual;
HMENU real/ocho grupos/IDs/separadores/Alt/F10/seis macros, labels y capabilities;
seis reentradas fuente y tres campo, wrong-group/disabled; composición/compact/
hits/jobs y limpieza Source13B identidad/revisión/encoding/catálogo/disco. Metadata
Git propia intacta; un WM_PAINT, cero errores. Native aprobó primera corrida;
24ms del arnés no es latencia física. Popup y clipboard son transporte sintético.

fmt, Clippy workspace/all-targets `-D warnings`, release workspace bins/examples
aprobados offline, Rust1.85.1 MSVC x64. Fallo inicial retenido de tres lints solo
del arnés, corregidos juntos. [Logs/hashes/nombres únicos](evidence/m2b-native-interaction-2026-11-03-metadata.json),
[reporte](evidence/m2b-native-interaction-2026-11-03-native-report.json),
[scroll](evidence/m2b-native-interaction-2026-11-03-native-interaction.png),
[restauración](evidence/m2b-native-interaction-2026-11-03-native-frame.png).
PNG1384×741/984×641 revisados visualmente, derivados sin pérdida de GDI. Solo área
cliente, no screenshot de menú desplegado. Find→menú cancelado cubierto;
paleta→macro desde Find no tuvo caso separado. Sin clipboard privado leído/alterado.

[ZIP verificado](evidence/m2b-native-interaction-2026-11-03-package.json): CRC/hashes69
entradas. Aceptación física/global/Unicode/OS clipboard/IME/DPI/accesibilidad/Linux,
RAM o latencia global, symlink y dos notices GNU siguen pendientes. Paquete local
de desarrollo, sin instalador/distribución final. Sin nuevos proveedores/AVM/
dependencias/regex/instalaciones/seguridad/CI/push/publicación.

## Seguimiento paleta/menú con Find/Replace — 2026-11-03

[Alcance auditado](M2B_MENU_INVOKER_VALIDATION.md). Un check nuevo, con siete
checks comunes de arranque/pintura; sección menu-invoker aprobada a la primera.
Tres estados de campo × seis macros × dos cancelaciones base (36 subflujos,
no36 pruebas distintas), continuación WM_CHAR/Backspace en campo, toggle View
permitido/revertido y Duplicate rechazado sin editar fuente. Ocho reentradas con
texto/caret/campo cambiados descartan la elección tardía. Draft Unicode/emoji/CRLF,
carets bytes/modo/generación, Source13B identidad/revisión/encoding/dirty/undo/redo,
vistas/split/pin/catálogo/disco y preferencia conservados; query inicial restaurada,
scan cancelado. Source13B ASCII jamás se edita: no otra matriz de BOM/legacy aquí.
Un WM_PAINT, cero errores;16ms del arnés no son latencia física.

Tres rechazos CLI por rutas faltantes, sección duplicada o mezclada, exit2.
Fmt, ClippyGUI/all-targets -D warnings y releaseGUI bins offline/MSVC aprobados.
Cero ejecuciones unitarias ni suites/perfiles históricos repetidos. Construcción
y cableado completos antes de checks, sin fallos/correcciones de producto/oráculo.
[Reporte](evidence/m2b-menu-invoker-2026-11-03-native-report.json),
[logs/hashes](evidence/m2b-menu-invoker-2026-11-03-metadata.json),
[CLI](evidence/m2b-menu-invoker-2026-11-03-cli.json).
Elección HMENU y clipboard sintéticos, no clipboard privado ni pantalla/menú físico.
Handlers productivos intactos frente a171af05; sin paquete nuevo. Los gestos mouse
horizontal/autoscroll/word-selection/campos continúan sin admisión de construcción.
Puertas físicas/globales/Unicode/OS/IME/DPI/accesibilidad/symlink/notices abiertas;
sin dependencias/proveedores/AVM/terminal/regex/instalaciones/seguridad/CI/push nuevos.

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
## Enlaces Markdown y esquema ABL — 2026-11-03

Las suites dirigidas de servicios de fuente de `jc-app` y `jc-progress`
pasaron 15 casos, incluidos selección del enlace bajo el cursor, apertura
lateral de un archivo Markdown en Windows y extracción de declaraciones ABL.
Pasaron también 38 pruebas unitarias filtradas de aplicación, GUI y TUI, una
prueba de destinos relativos y una prueba de navegación del Esquema que rechaza
clics sobre filas obsoletas. Formato, Clippy de todo el workspace y compilación
de ejecutables en modo release aprobados. Binarios en `target/source-next/release`.
El renombrado por ámbito sigue pendiente de resolución semántica.
Ver [enlaces y esquema](ENLACES_ESQUEMA_RENOMBRADO.md).

## Navegación por proveedor — 2026-11-03

Pasaron 40 pruebas dirigidas de aplicación, ABL, JavaScript, GUI y TUI.
Una prueba de integración abre el destino ABL en el grupo lateral y comprueba
el orden de dos entradas PROPATH con el mismo nombre de archivo. Formato y
Clippy de todo el workspace aprobados. El alcance del analizador y la
configuración se describen en [Navegación por proveedor](NAVEGACION_MULTILENGUAJE.md).

## Navegación inversa y desplazamiento de los editores — 2026-11-03

Pasaron 17 pruebas de servicios de fuente. Los casos de navegación inversa
verifican `RUN` e inclusiones con PROPATH, importaciones estáticas JavaScript,
apertura lateral y rechazo ante cambios en el origen o la resolución.
Pasaron siete pruebas de ratón TUI y una prueba dirigida de barras y minimapa
GUI. Las pruebas comprueban que el arrastre mueve la vista y conserva el cursor.
Clippy y formato aprobados. El minimapa GUI muestra una muestra acotada del
documento y se oculta en paneles estrechos. La barra horizontal mide el
contenido visible dentro de su presupuesto. Compilación optimizada aprobada
en `target/editor-navigation-next/release`.

El intento de suite completa de TUI encontró una prueba que requiere crear
una unión de directorio y recibió acceso denegado. Al omitirla, la suite
terminó con `0xc0000409` en una prueba de integración de estado Git. No se
acredita una pasada completa de la TUI.

## Renombrado ABL de variables — 2026-11-03

Pasaron dos pruebas dirigidas del analizador ABL, 17 de servicios de fuente
y 24 pruebas filtradas de GUI y TUI. Los casos nuevos cubren sombreado local
y global, colisiones, comentarios y cadenas, confirmación explícita, un solo
deshacer y conservación de CP1252 y CRLF. Formato y Clippy de todo el
workspace aprobados. Compilación optimizada en `target/rename-next/release`.
La evidencia es local en Rust. Ver
[subconjunto admitido](ENLACES_ESQUEMA_RENOMBRADO.md).

## Renombrado ABL de rutinas internas — 2026-11-04

Pasaron seis pruebas dirigidas del analizador, GUI y TUI. Los casos nuevos
cubren `RUN` estático, llamadas anidadas a funciones, colisiones con nombres
de variables y rutinas, rechazo de llamadas dinámicas, vista previa sin
mutación y deshacer de toda la propuesta con conservación de CRLF.
Clippy del workspace y formato aprobados. El análisis no se ha validado
contra el compilador OpenEdge.

## Renombrado ABL con FORWARD — 2026-11-04

Pasaron ocho pruebas dirigidas de renombrado. Los casos nuevos verifican
que prototipo, implementación y llamadas producen los mismos rangos, que
los parámetros omitidos en la implementación se resuelven desde el prototipo
y que las firmas incompatibles o sin contraparte se rechazan. La prueba
TUI revisa los cuatro cambios antes de confirmar y recupera el archivo con
un solo deshacer. Formato y Clippy aprobados. Validación local en Rust.
