# M2A — Selección rectangular de una acción

**Actualización vigente:** [M2_UNICODE_LAYOUT](M2_UNICODE_LAYOUT.md) reemplaza la
whitelist de una celda por grafemas narrow/TAB4, define cortes parciales de glifos
anchos y conserva transacciones/cuotas. El cuerpo siguiente es el checkpoint
histórico de admisión, uso y evidencia de la primera versión; sus exclusiones de
CJK/emoji/combinantes ya no describen el comportamiento actual.


**Seguimiento GUI/presentación:** [M2B_RECTANGLE_GUI](M2B_RECTANGLE_GUI.md) completa
las rutas nativas y reemplaza la presentación rectangular con invisibles por una
cuadrícula TAB4 compartida. El alcance Unicode acotado de este contrato se conserva;
las referencias a backend gráfico pendiente describen el checkpoint anterior.

Implementada en el modelo compartido y la TUI, con alcance explícito de texto de
una celda. No equivale todavía al modo columna Unicode general previsto en el
plan: grafemas, ancho doble, emoji y backend gráfico continúan pendientes.

## Uso y alcance visible

En Editor, Alt+Shift+flechas inicia/extiende un rectángulo por celdas visuales y
líneas lógicas cuando el terminal entrega esas combinaciones. Edit y Palette
ofrecen las cuatro extensiones, Cancel y `Rectangle: Select From Caret/Selection
(single-cell text)`. Este último convierte los extremos de una selección lineal
existente, o comienza sin ancho en el caret; permite preparar ambos extremos con
selección normal aunque el terminal no entregue Alt+Shift. No hay switch decorativo.
Los comandos de rectángulo están deshabilitados con foco en otros controles.

Copy/Cut usan el clipboard interno existente de Workbench, ahora tipado como
texto lineal o filas rectangulares. No se añadió integración con el clipboard
del sistema operativo. Tab/entrada de texto y pegado reemplazan las filas
seleccionadas en una sola transacción. Una fila se repite en todas las filas del
rectángulo; varias deben coincidir exactamente con su altura. El paste de un
clipboard rectangular en un caret inserta filas sobre líneas existentes desde
esa posición: no crea líneas después de EOF. Se rechazan alturas incompatibles
sin editar ni alterar el historial. CRLF/LF/CR del texto pegado separan las filas
del clipboard, no sustituyen los separadores originales del documento.

Una edición efectiva termina la selección rectangular; no se promete multicaret
persistente para cada pulsación posterior. Delete/Backspace sin ancho borran una
celda a derecha/izquierda por fila, sin unir líneas. Borrar solo espacio virtual
es no-op y no crea undo. Enter, outdent y los comandos de línea/comentario requieren
salir antes del rectángulo. Las flechas normales vuelven a selección lineal;
Escape cancela el rectángulo y la búsqueda documental pendiente mediante el
comando central ya existente. Cancel en Edit/Palette cancela solo el rectángulo.

## Geometría y bytes

`rectangle::VisualColumn` distingue columnas visuales de posiciones escalares y
bytes. `LineLayout` calcula offsets absolutos desde `DecodedScalars`, sin conversión
lossy. Stops TAB cada cuatro celdas. El repertorio cerrado es ASCII imprimible,
U+00A0…U+00FF excepto soft hyphen, más los 19 signos de puntuación/símbolos CP1252
enumerados en el código. No supone que todo CP1252/IBM850 o todo escalar Unicode
ocupe una celda. Controles, bytes inválidos, combinantes, CJK, emoji, ZWJ y cualquier
otro carácter fuera del repertorio producen un error antes de activar/extender
o editar las filas afectadas. La validación examina la línea completa, incluso si
el carácter no admitido queda fuera de las columnas seleccionadas.

Copy convierte TAB completos/parciales seleccionados a espacios y rellena columnas
virtuales: pegar en otra fase de TAB conserva el ancho copiado. Las ediciones
preservan los TAB fuente completamente fuera del corte; solo materializan espacios
en los TAB atravesados. Insertar en una columna virtual rellena hasta esa columna;
borrarla no extiende el documento. Reemplazos por fila usan rangos de bytes
excluyendo BOM/EOL y encoding estricto. Se revalida también el ancho resultante,
incluida la nueva fase de los TAB posteriores, antes de aplicar.

No hay tablas de grafemas/anchos ni dependencias nuevas. Este contrato conserva la
geometría determinista del Screen lógico; no certifica que toda terminal/fuente
dibuje el repertorio igual. El modo columna general sigue sujeto a un layout de
grafemas y display compartido; no se debe ampliar la whitelist para simularlo.

## Propiedad y transacciones

Cada ViewState posee un único cursor y anchor en bytes. Su metadata rectangular
añade columnas visuales del anchor/head, scroll visual independiente del scroll
escalar y revisión. Las líneas se derivan de los offsets existentes; no hay otro
caret ni lista persistente de rangos. Split conserva selecciones independientes
sin copiar Document, FileSession, bytes o historial.

`rectangle_for_pane` entrega límites visuales solo para la revisión vigente.
`selection_for_pane` delega en `ViewState::selection`: un rectángulo jamás aparece
también como rango lineal. Navegación y resultados de búsqueda reemplazan
explícitamente la selección. Cambios de contenido (incluidos propuestas externas,
undo/redo) y de encoding descartan la metadata rectangular afectada en ambas
vistas y limpian su anchor, evitando una selección lineal residual.

Las filas propuestas se validan y codifican antes de llamar a `apply_text_edits`
con DocumentId y revisión esperada. Un batch publica un ChangeSet completo y una
sola entrada de undo; las demás vistas usan su mapa. No se escribe disco ni se
normalizan BOM, separadores, encoding o newline final. Cut solo reemplaza el
clipboard tras completar la transacción; errores conservan bytes/revisión/undo.

## Trabajo y memoria acotados

- Hasta 1024 filas, columnas de edición 0…4096 y 64 KiB fuente por línea.
- Hasta 2 MiB de fuente de las filas por operación; se comprueba con metadatos
  antes de generar propuestas. Se procesa una fila por vez.
- Clipboard rectangular y staging de propuestas tienen límites de 2 MiB de
  cargo lógico por separado. No son una garantía de RAM privada: existen
  capacidades de contenedores, clipboard previo, propuesta codificada y batch.
- Layout de una línea limitado además por 4096 celdas; metadata temporal por
  átomos, sin String de toda la fuente ni caché permanente de rectángulos.
- Extender sobre filas validadas en la misma revisión reutiliza esa admisión;
  solo decodifica filas nuevas y la fila de destino. La preparación decodifica
  las líneas visibles acotadas, no todo el rectángulo/documento. No hay benchmark
  de latencia de este flujo ni declaración de mejora general.

La presentación expande TAB y padding virtual antes de componer. El sentinel
`VIRTUAL_BYTE` solo identifica padding de pantalla: nunca es offset de edición
ni participa en highlights de búsqueda. Render consume filas prestadas y no
muta geometría, vistas o cachés. El viewport se actualiza antes del render y el
encoder incremental existente continúa emitiendo solo cambios.

## Evidencia local — 2026-11-02

Bloque completo construido antes de validar. Seis pruebas puras de geometría,+diez de Workbench y cuatro de Screen nuevas: **20 regresiones**. Incluyen TAB
parcial/completo, virtual, UTF-8/1252/ISO-8859-1/IBM850, encoding estricto,
rechazos Unicode/controles, límites, copia/corte/pegado, undo/redo único, split,
propuesta externa, BOM y bytes inválidos fuera del rectángulo, mixed EOL y ausencia
de escrituras; Screen 45/140, resize, selección/cursor visual e idempotencia.

La primera corrida workspace pasó 142 casos antes de fallar una nueva aserción:
`selection_for_pane` aún producía un rango lineal para el rectángulo. Se retiró
ese cálculo duplicado. La corrección, las vistas compartidas y los tests de
aplicación pendientes se validaron con 53 pruebas dirigidas; después pasaron
138 de los otros crates, incluidos 60 de TUI y cinco del CLI TUI. Hasta ese punto: **309 pruebas distintas aprobadas**, no una corrida completa única de 310.
Dos helpers ignorados se invocan desde sus padres. No se repitió la suite costosa
ya aprobada ni los perfiles CPU/índice. Formato y Clippy workspace all-targets
con warnings denegados y compilación release workspace bins/examples aprobaron. Sin nuevo recorrido ConPTY ni pantalla física; sin CI/push/AVM.

Revisión final: extender dentro de TAB/virtual puede cambiar intención visual sin
mover cursor/anchor en bytes. Se descarta la navegación documental diferida en esa
acción; una décima prueba Workbench verifica que la entrega de find no reemplace
el rectángulo. Pasaron las diez pruebas de rectangle, Clippy dirigido de app y
release actualizado. Ese caso adicional completa las **310 distintas**; la fase
anterior de 142 + cierre dirigido + otros crates había reunido 309. No se repitió
la suite global por esta corrección localizada.
