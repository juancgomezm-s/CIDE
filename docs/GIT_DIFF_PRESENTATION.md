# Git diff — Composición TUI de solo lectura

Bloque del 2026-11-02. Componente preparado en
`crates/jc-tui/src/render/git_diff.rs`, conforme a
[GIT_DIFF_DELIVERY_CONTRACT](GIT_DIFF_DELIVERY_CONTRACT.md). Este worker posee
únicamente ese archivo y este documento; el integrador declara el módulo,
prepara datos y conecta comandos/controlador. No se modifica la escena GUI ni
se reabre una superficie de navegador.

## Entrada y propiedad

Las funciones `sidebar(screen, &prepared, x, y, width, height)` y
`unified(screen, &prepared, x, y, width, height)` consumen exclusivamente
`GitPresentation` preparado por la aplicación:

- `status` y `details`: estado e identidad ya formateados, como ruta, revisión y
  snapshot/OID. El compositor no consulta si HEAD/index cambió posteriormente.
- `hunks`, `total_hunks`: filas visibles y total del modelo; cada fila contiene
  índice original, label y selección preparada.
- `rows`: prefijos textuales visibles, tipo de fila, números de línea one-based
  opcionales y token de EOL. No contiene un documento editable.

No recibe Workbench, FileSession, modelos Git/diff ni snapshots de bytes. No llama
Git, no lee disco, no decodifica, no calcula un diff ni mueve caret/selección.
Las únicas mutaciones son celdas del `Screen` recibido; conserva incluso su cursor.
La virtualización y los presupuestos de datos pertenecen a preparación/entrega,
no a una copia del archivo completo retenida en el renderer.

## Salida y geometría

Ambos componentes muestran `Git diff (read-only)`, estado y las líneas de identidad
que quepan. Se reserva espacio para las filas del panel antes de dibujar detalles;
un área de una sola celda/fila no puede mostrar todos esos campos.

Sidebar añade el total de hunks y sus labels numerados. La fila seleccionada usa
`>` y estilo Reverse en toda su anchura. Si el área se reduce, se deriva un inicio
de dibujo que mantiene visible la selección recibida; no se cambia `selected`, el
índice del controlador ni la lista preparada. Un fragmento vacío indica `No visible
hunks` cuando queda espacio, sin declarar que el cálculo completo no tenga cambios.

El panel unificado dibuja:

| Tipo de fila | Representación |
|---|---|
| Context | Espacio, números izquierdo/derecho, texto y EOL |
| Deleted | `-`, número izquierdo, texto y EOL |
| Added | `+`, número derecho, texto y EOL |
| Header | Texto preparado de hunk/cabecera, Dim |
| Notice | Aviso preparado, Dim |

Los EOL llegan como tokens **sin corchetes** `LF`, `CRLF`, `CR` o `no EOL`;
Header/Notice usan vacío. El renderer añade `[token]` y reserva su lugar a la
derecha. El final de un prefijo textual largo se recorta antes que ese badge, para
que un cambio exclusivo de EOL no quede oculto por la longitud del contenido.
La preparación debe entregar el token correcto; el renderer no lo deduce del texto.

Con al menos 28 celdas se dibujan dos columnas de números y separador. En áreas
más estrechas se conservan signo/texto/EOL en una forma compacta. Si ni siquiera
cabe el badge completo, se recorta; se prioriza mantener el signo en la primera
celda. Solo se usan Normal/Reverse/Dim, sin depender de colores para distinguir
altas y bajas.

`Area` intersecta el rectángulo con la pantalla antes de escribir, rellena solo su
interior y limita cada cadena a esa anchura. No usa `Screen.text` como sustituto
del clipping del subrectángulo: ese método únicamente recorta contra la pantalla.
Origen fuera de pantalla, dimensiones cero y rectángulos parcialmente visibles
no escriben en celdas adyacentes. El componente no hace wrapping de una fila en otra.

El ancho sigue la representación escalar/celda existente de `Screen`. No se añadió
un motor de grafemas, shaping ni ancho visual de caracteres combinantes/dobles.
La sanitización de tabs/controles es la propia de `Screen`; no es una conversión
de los bytes canónicos de la fuente. La aceptación Unicode de ancho gráfico y
pantalla física continúa fuera de este componente.

## Validación del bloque

Se aplicó rustfmt **solo a este archivo**, mediante el binario local Rust 1.85.1,
y `git diff --check` terminó sin errores. No se ejecutó Cargo, fixtures Git ni
benchmarks por este worker, para conservar la ventana serial del integrador.

Cuatro tests preparados dentro del componente:

1. En 45 y 140 columnas permanecen estado/identidad, cabecera, números one-based,
   signos y badges EOL; una fila larga no oculta `[no EOL]`.
2. Un sidebar de cuatro filas mantiene visible el hunk seleccionado y su estilo
   sin modificar la selección recibida.
3. Ambos componentes conservan las celdas exteriores y cursor para rectángulos
   normales, parciales, de una celda, de tamaño cero y con origen extremo.
4. Dos composiciones producen la misma pantalla y preservan campos, contenido e
   identidad de selección de la presentación preparada.

El integrador conectó módulo, comandos centrales y navegación. Las cuatro pruebas
del compositor y las tres de preparación pasaron dentro de 237 pruebas locales;
fmt/Clippy/release aprobados. Preparación recibe presupuestos independientes de
sidebar y panel inferior: ocultar este último conserva los hunks laterales. La
composición reserva todas las filas preparadas antes de detalles; un panel con
una única fila de contenido omite la cabecera para permitir recorrer los datos.
ConPTY verificó HEAD/index asíncronos, navegación, undo→Stale y ocultar/reabrir
el panel sin perder hunks: [datos crudos](benchmarks/M2-git-conpty.json).

Esto no acredita una GUI, operaciones de escritura Git, aceptación completa de
M2A ni metas globales de RAM o latencia. No se añadió dependencia, otro editor/undo
ni backend gráfico. La memoria del harness se toma antes de activar Git.
