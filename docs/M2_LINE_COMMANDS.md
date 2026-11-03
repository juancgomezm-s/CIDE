# M2A — Unir y dividir líneas

Bloque integrado y validado localmente: dos operaciones universales, sin reglas
por lenguaje, servicios externos ni otro motor de edición. El núcleo comparte
la misma ruta transaccional con teclado, menú y Palette.

## Comportamiento

`Join Lines` une la línea actual con la siguiente línea real cuando no hay
selección. Con selección lineal une las líneas completas tocadas, excluyendo la
última si el extremo final coincide con su comienzo. Seleccionar parte de una sola
línea es no-op. Nunca consume el registro vacío sintético tras el newline final,
por lo que conserva ese newline. Se retiran los EOL internos y la indentación
ASCII inicial (SP/TAB) de las líneas siguientes; se inserta un espacio ASCII
cuando hay contenido acumulado sin whitespace final y contenido siguiente.
Las líneas intermedias vacías no concatenan palabras: `a\n\nb` resulta `a b`.
La indentación de la primera línea y el EOL de la última permanecen intactos.
Es una transformación textual explícita, no un formateador sintáctico.

`Split Line` reutiliza `insert_newline`: inserta el EOL preferido del documento
en el caret o reemplaza la selección lineal con ese EOL, usando encoding estricto
y el undo existente. No es otro algoritmo de corte ni otra política de EOL.
Ambas acciones requieren documento/encoding y rechazan selección rectangular.
Se ofrecen en Edit y Palette; Enter en Editor resuelve el mismo CommandId de Split.
Join usa Ctrl+Shift+J en Editor. Ctrl+J conserva el panel inferior existente.
Otros controles y overlays conservan su semántica de Enter.

## Implementación y límites

La clasificación raw byte→ordinal de línea usa el índice de líneas por revisión,
sin construir columnas escalares o visuales. Esa API no valida un caret: un byte
dentro de un escalar/CRLF sigue siendo solo un offset clasificado. Join la usa
con extremos ya poseídos por la vista y rangos de metadatos; el BOM no se incluye
en los cortes. Source encoding es explícito y los bytes fuera de EOL/indentación
no se decodifican/reconstruyen para preparar propuestas.

Antes de generar el plan se admiten hasta 1024 fronteras y 2 MiB del tramo fuente
afectado, calculados desde metadatos. Se examinan prefijos en rangos de hasta
4096 bytes y una cola de un byte para decidir el espacio. Un plan contiene solo
rangos EOL/prefijo y strings vacíos o de un espacio. Se aplica un único
`apply_text_edits(DocumentId, expected_revision, edits)`: una revisión, un undo,
mapas para las vistas y ninguna escritura al archivo.

El límite de preparación no promete que toda la actualización F2 lea solo esos
prefijos: la validación textual, layout frío, índices y normalización de carets
siguen por la ruta existente y pueden visitar líneas afectadas o de otras vistas.
No se crea una ruta de mutación sin validación para evitar ese coste. No hay nueva
medición de rendimiento ni aceptación de RAM/latencia física en este bloque.

## Evidencia local — 2026-11-02

Cinco pruebas nuevas de Workbench cubren mixed EOL, indentación/vacíos, selección
por líneas/fin en comienzo de otra línea, terminal newline y EOF, UTF-8/BOM/bytes
inválidos preservados, Windows1252/IBM850, disco sin cambios, undo/redo compartido,
Split y rechazo rectangular, cuotas/exacto1024. Cuatro nuevas de TUI ejercitan
Join por Ctrl+Shift+J y Palette; Split por Enter/menu/Palette; reemplazo de selección,
encoding/BOM, una revisión/undo, vistas compartidas y Screen45/140 readonly;
scope de otros controles/overlays y Ctrl+J; rechazo rectangular en todas las vías.

Bloque construido completo antes de comprobar. Pasaron **74 pruebas dirigidas
de jc-app**. La corrida TUI aprobó 63 y falló una nueva por asumir que Symbols,
todavía deshabilitado, podía recibir foco. Se corrigió solo el fixture a Explorer
vacío y el recorrido real de F6; sus cuatro pruebas nuevas aprobaron después.
Son **319 pruebas distintas aprobadas** al sumar las nueve nuevas al checkpoint
anterior, no una corrida única de 319 ni una repetición de todas las anteriores.
Formato, Clippy workspace all-targets con warnings denegados y release workspace
bins/examples aprobaron. No hubo cambios productivos derivados de ese fixture.
Sin CI/push/instalaciones ni repetir perfiles CPU/índice. Screen lógico no acredita
nuevo ConPTY, GUI, RAM global ni latencia física.
