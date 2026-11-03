# Git status — componente de presentación TUI

Estado integrado y validado localmente en Windows: comandos, controlador, foco,
preparación visible, composición y despertar del loop conectados. Una corrida
completa aprobó 290 pruebas; Clippy all-targets y release bins/examples aprobaron.
No acredita pantalla física ni aceptación global de memoria/latencia.

## Uso

Abra la raíz del repositorio con Open Folder y seleccione View / Git: Repository
Status, también disponible en la palette. No requiere documento abierto ni
encoding de fuente. Si falta Git, solicita una ruta absoluta de ejecutable para
la sesión y continúa la consulta de status al aceptarla. La configuración manual
sin intención pendiente no inicia una consulta; Escape descarta esa intención.
Refresh y Cancel son comandos centrales del menú/palette. Up/Down/Home/End
seleccionan filas; Enter no abre archivos. La observación corresponde a disco e
índice, no incorpora buffers sin guardar y no es una transacción del filesystem.

## Propiedad y flujo

`jc-tui/src/app/git_status.rs` define `GitStatusView`, que posee un único
GitStatusController, opciones de contexto, selección y scroll. No introduce otra
lista persistente de rutas, clon del snapshot, Document, FileSession ni historial.
Los resultados continúan siendo observaciones inmutables del
[proveedor/controlador existente](GIT_STATUS_READ_ONLY.md).

`new(Wakeup)` no inicia trabajo. `request(root, executable)` comprueba solo
metadatos acotados y configura el controlador bajo demanda; el proceso/I/O ocurre
en su worker. Rutas relativas o mayores de 4096 unidades se rechazan sin iniciar
el proveedor. Refresh vuelve a solicitar con el contexto actual, reiniciando
selección/scroll y descartando la observación anterior. No hay watcher ni consulta
automática al pintar.

`poll(root, executable)` es la única ruta que consume completions y devuelve
cambio visible. El integrador la llama en poll_tasks antes de preparar; si devuelve
cambio, actualiza la presentación; además consume `take_recovery()` para invalidar
el encoder incluso si un pánico pertenece a una generación ya descartada. El hook
puede haber alterado la terminal antes del rechazo de esa entrega. `ready()`
permite que el loop detecte backlog.
`state()` y `message()` ofrecen estados/mensajes acotados para el contexto de
comandos y la barra de estado.

`validate_context(root, executable)` solo compara metadatos, cancela/descarta
opciones/resultados cuando difieren y devuelve cambio. Se reutiliza en poll y se
llama también antes de preparar, **sin aceptar completions**. Así un resultado
aceptado tampoco sobrevive al cambio/desaparición de raíz o Git; no se abre una
segunda ruta de entrega que eluda la recuperación de pánicos. No canonicaliza ni
revalida disco desde preparación.

`cancel()` descarta resultados/pendiente, conserva contexto únicamente para
detectar su invalidación y muestra Cancelled; `clear()` vuelve a Idle y elimina
el contexto de la vista. No se expone refresh del controlador con opciones viejas:
la nueva solicitud siempre recibe raíz/ejecutable actuales. La cancelación real
del proceso y eventual limpieza conservan sus límites documentados del proveedor.

## Lista y contrato con renderer

Up/Down llaman `move_selection(isize)`, Home/End llaman `first()`/`last()`.
Se saturan extremos y se limita la selección a entradas vigentes. Es selección
de una fila: **no** abre ni navega rutas de Git, no lee otro archivo y no propone
edición/guardado. Enter no tiene una operación de archivo asociada a este estado.

`prepare(width, list_height)` normaliza el viewport y devuelve únicamente:

```text
GitStatusPresentation { title, message, rows, total, selected, scroll }
GitStatusRow { ordinal, flags, path, selected }
```

Los campos de texto/filas son propios de esa presentación, leídos por renderer;
total/selección/scroll son ordinales de lista, nunca posiciones de documento.
Solo se formatean las entradas del intervalo visible; saltar a la última de
4096 entradas no crea strings de las anteriores. Como defensa ante geometría
absurda, el máximo es 256 filas y 256 columnas por preparación.

El renderer reserva cuatro celdas de prefijo (marker, dos flags y espacio);
el path recibe `width.saturating_sub(4)`. Los flags siempre son dos ASCII:
index/worktree con `.` sin cambios, `??`, `!!` o el par específico de conflicto.
Conflictos conservan esa categoría, sin prometer que sus dos letras son estados
staged/unstaged normales.

Un único helper compartido, `git_status_list_height(height)`, descuenta headers:
altura 0 → lista 0; altura 1 → lista 1 y sin header; altura 2 → lista 1 y un
header; altura ≥3 → lista height−2. Root pasa ese resultado a prepare y renderer
reutiliza el helper. No se descuentan dos veces las filas de encabezado.

## Rutas y mensajes

El formateador interpreta como máximo cuatro bytes por token Unicode; no valida
un path completo para obtener su prefijo corto. Conserva Unicode UTF-8 válido y
escapa cada byte inválido como `\xNN`. Escapa también controles, newline/tab/CR,
backslash, bidi y caracteres de formato seleccionados. Las secuencias de escape
son indivisibles al recortar; la elipsis indica bytes sin mostrar. No utiliza
conversión lossy, unquote ni conversión a PathBuf. Un backslash literal queda
distinguible del escape de un byte/control.

El ancho sigue el modelo actual de celdas/escalares de la TUI; no acredita ancho
real de grafemas, emoji o CJK en todas las terminales. Las rutas mostradas son
prefijos de destino para inspección, no autoridad de filesystem ni descripción
completa de un rename. Los bytes originales permanecen solo en el read model.

El título `Git status [iso]` identifica configuración global/system aislada.
El mensaje añade `no renames/submodules` además del estado, sujeto al clipping
del rectángulo; la barra de estado puede consumir el mensaje de estado completo.
El alcance continúa siendo el del transporte: sin detección de renames y sin
estado completo de submódulos. Cero filas reportadas no se presenta como una
garantía de repositorio globalmente limpio ni igualdad con el Git personal.

ConfiguredFilter se reconoce por su variante tipada y muestra `Filters
unsupported`; el resto de fallos usa mensajes estáticos. No se formatean stderr,
paths de error ni payloads de pánico en pantalla. No se instala, invoca ni
suprime silenciosamente un filtro para mostrar esta lista.

## Regresiones y cierre local — 2026-11-02

Cinco tests del componente cubren Unicode, bytes inválidos y controles/bidi,
backslash y clipping sin medias secuencias; viewport sobre 2000 entradas y resize
incluida geometría cero/absurda; ocultación de filas previamente aceptadas al
cambiar/desaparecer raíz o ejecutable y tras cancel; estados idle/clear/cancel y
rechazo de request relativo sin proveedor; altura compartida, flags y mensaje de
filtros. Los fixtures son snapshots obtenidos del parser puro, sin procesos Git
ni disco. La prueba de filas aceptadas ejercita el estado/presentador con esos
snapshots; no inventa una API pública de inyección del controlador cross-crate.

Cuatro tests del renderer cubren Screen de 45/140 columnas, clipping, rectángulos
pequeños/extremos, selección visible e idempotencia sin mutar la presentación.
Cinco integraciones con Git real cubren consulta sin documento/encoding, entrega
sin otra tecla y aceptación solo en poll_tasks; bytes legacy sucios y split con
sesión/undo/carets únicos; configuración con intención status/diff y Escape;
invalidación por raíz/ejecutable, cancelación/refresh y rechazo de filtros. Comparan
disco, index, config, HEAD y ref de rama, además del estado del editor. Una prueba
de registro comprueba disponibilidad sin documento y una del controlador cubre
recuperación de terminal tras pánico de generación obsoleta: 16 pruebas nuevas.

La corrida completa del workspace aprobó **290 pruebas, cero fallos**, con dos
helpers ignorados invocados por sus tests padre. Formato, Clippy workspace
all-targets con warnings denegados y release workspace bins/examples aprobaron.
No se ejecutó CI ni se repitieron los perfiles CPU ya cerrados. Screen lógico y
procesos Git nativos no acreditan GUI ni latencia de pantalla física; no se declara
Git con escritura, aceptación global de RAM ni cierre completo de M2A.
