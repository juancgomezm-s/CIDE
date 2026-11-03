# M2A — Quick Open de archivos del proyecto

Estado: implementado e integrado en TUI, con validación automatizada local.
Es el pendiente Ctrl+P de M2_NAVIGATION_PANES_GIT, no navegación semántica M3.
Sin dependencias nuevas, indexador permanente, regex, AVM o lecturas de contenido
durante la consulta.

## Flujo

Abra una carpeta de proyecto y use Ctrl+P, File / Quick Open Project File o Palette.
El comando funciona sin documento ni encoding de fuente. La consulta es una
subcadena literal de la ruta relativa, insensible mediante full folding Unicode17;
normaliza slash/backslash solo para comparar, sin modificar la ruta autoridad.
No es ranking fuzzy. La consulta vacía lista un prefijo de archivos; el orden es
el del recorrido del filesystem, no una clasificación global por relevancia.

Up/Down, PageUp/PageDown y Home/End seleccionan. Enter abre, Ctrl+Enter abre al lado,
F5 vuelve a consultar y Escape cancela/cierra. Cambiar consulta cancela la anterior;
una entrega vieja no reaparece. La lista Partial/Limited requiere refinar y puede
abrir sus candidatos visibles tras revalidación, sin afirmar cobertura global.

Aceptar revalida raíz/consulta/generación y metadatos actuales, rechaza targets
borrados, links/reparse o fuera de la raíz. Abrir un documento ya registrado
reutiliza bytes sucios, sesión, undo y vistas; no lo recarga desde disco. Un nuevo
archivo usa FileSession::open_in_root: validación del handle antes de leer bytes,
límite de16MiB incluso si crece durante la lectura. Ese límite es propio de Quick
Open; la apertura explícita normal mantiene su contrato previo. Una fuente legacy
ambigua abre byte-preserved y pide la selección de encoding existente en TUI.

## Recursos y entrega

Un worker lazy reutilizable, un trabajo activo, última intención pendiente de
metadatos y un completion; sin snapshots de documentos. Wakeup despierta el loop
sin otra tecla. Solo poll_tasks consume completions. Preparación descarta contexto
de raíz obsoleto pero no acepta entregas; render lee únicamente filas visibles.
Cancelar/cerrar libera resultados y pendiente; Drop cancela y une el worker.
La señal de recuperación terminal sobrevive a un pánico de generación descartada.

Cuotas predeterminadas:256 bytes UTF8 de consulta,4096 unidades de path,
profundidad32,50000 entradas examinadas,512 resultados,512KiB de capacidades
retenidas del resultado y deadline cooperativo de2s desde admisión. El recorrido
retiene hasta32 ReadDir y metadatos acotados aparte; buffers del SO/allocador no
son una garantía de RAM privada. Una syscall de filesystem bloqueada puede
retrasar cancelación/Drop; no se promete deadline duro ni rendimiento físico.

No se siguen links/reparse; .git se excluye. No se leen .gitignore ni contenido
para clasificar archivos, tampoco se ejecutan herramientas externas. Archivos
sin contenido de texto siguen siendo candidatos; byte-preserving no presupone
encoding por extensión. Paths no Unicode aparecen escapados con consulta vacía;
con consulta textual no vacía se omiten con limitación explícita. El label es un
prefijo seguro de display, no se convierte de vuelta a ruta ni autoridad de open.

## Apertura confinada compartida

El lector por handle de Search Plus se trasladó a jc-files/scoped_read.rs, y la
ruta de Search Plus delega en él. Conserva validación de reparse, canonicalización,
comprobación del handle (Windows/Linux), cuota de bytes y sentinel EOF en stack.
FileSession construye el mismo Document/fingerprint desde esos bytes, sin otro
editor. Plataformas sin comprobación equivalente fallan cerradas en esa API.
La revalidación de metadatos del selector no se presenta como sustituto de la
comprobación del handle, ni como confinamiento permanente de guardados futuros
frente a cualquier cambio externo del filesystem.

## Validación de cierre prevista

Regresiones observables de controller/fixtures: Unicode/casefold, entradas/cuotas,
supersession/stale/pánico/cancel/Drop, links y aceptación. TUI: Ctrl+P/Palette/menu,
entrega sin input y prepare sin consumo, Screen45/140 readonly, legacy prompt,
Ctrl+Enter con documento sucio compartido/pin/undo/historial y targets rechazados.
Scoped reader: bytes exactos, presupuesto y rechazo de handle fuera de raíz.
Resultados de ejecución se registrarán al cerrar el bloque en VALIDACION.

## Cierre observado — 2026-11-02

Windows: cuatro unitarias del controller y cinco integraciones de filesystem,
seis integraciones TUI y dos del lector confinado aprobaron. La primera corrida
workspace pasó148 antes de fallar la creación privilegiada de un symlink del
fixture (Windows1314). Se cambió SOLO ese montaje a junction propia, como en
Search Plus: el rechazo real se ejecutó sin omitirlo ni cambiar privilegios.
Pasaron42 dirigidas de app y152 de los otros crates;338 distintas a ese punto.
El API de colocación de caret del GUI añadió luego una regresión:339 distintas
al cierre conjunto, no una corrida única339. Formato/Clippy/release aprobaron.

La prueba de path no Unicode específica de Unix no se ejecutó en Windows; no se
infiere aceptación Linux/macOS. El límite de path se mide con OsStr::len (unidades
codificadas internas de Rust), no offsets de fuente ni columnas visuales/UTF16.
No benchmark nuevo ni aceptación global de RAM/latencia. Quick Open de este
checkpoint es TUI; su adaptación al GUI constituye el siguiente bloque activo.
