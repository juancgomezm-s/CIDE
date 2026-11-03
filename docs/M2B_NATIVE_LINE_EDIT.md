# Líneas y comentarios GUI compartidos — 2026-11-03

Bloque posterior a navegación `41abf6c`. La GUI admite EditDuplicate,
EditDeleteLine, EditMoveLineUp/Down y EditToggleLineComment/BlockComment. Menú Edit,
keymap y paleta ejecutan el mismo registro/Workbench que TUI. No se crea otro
motor de edición, historial, buffer ni proveedor de lenguaje.

## Uso y semántica existente

| Comando | Atajo del keymap | Operación de Workbench |
| --- | --- | --- |
| Duplicate Selection/Line | Ctrl+D | Duplica selección exacta y selecciona su copia; sin selección duplica la línea del caret |
| Delete Line | Ctrl+Shift+D | Elimina la línea del caret, aunque exista una selección lineal |
| Move Line Up/Down | Alt+Up/Down | Mueve la línea del caret; no es un movimiento de bloque seleccionado |
| Toggle Comment | Ctrl+/ | Usa el comentario preferido del perfil; ABL usa delimitadores de bloque |
| Toggle Block Comment | Ctrl+Shift+/ | Usa delimitadores de bloque del perfil, sobre selección o contenido de línea |

Delete/Move limpian selección según su contrato existente. Las selecciones
rectangulares deshabilitan estas seis operaciones. Cada cambio real usa el undo
compartido existente; no guarda el archivo. Undo/redo pertenece al Document,
independiente del grupo que lo solicite. Pin protege navegación automática, no
convierte una vista en solo lectura: editar el grupo fijado edita su documento.

EOL y BOM siguen gobernados por los bytes y la proyección existentes. Duplicar
una línea EOF sin separador introduce el EOL preferido entre original y copia;
Delete Line en EOF puede retirar el separador anterior. Move conserva/mueve los
separadores según las líneas intercambiadas. El registro vacío EOF tras un EOL
cuenta como línea: mover `one\r\n` hacia abajo produce `\r\none`, reversible con
un undo. No afirmar que todo comando conserva el newline final sin cambios: esas
operaciones explícitamente modifican geometría de líneas.

Comentarios proceden de `jc-languages`. ABL usa `/* ... */`; C# tiene `//` y bloque,
Python `#` sin bloque. Sin capacidad correspondiente, el comando se muestra
deshabilitado y un despacho explícito devuelve indisponibilidad. No implica que
los auxiliares ABL se hayan compilado/ejecutado en OpenEdge. No se comprueban AVM,
parser, semántica de lenguaje ni comentarios dentro de strings.

## Foco y propiedad

`CommandContext.editor_input` indica si el control activo admite editar fuente.
El registro exige ese flag, foco Editor y selección lineal para estas seis
operaciones, más metadata del perfil para comentarios. El default mantiene los
consumidores existentes; la GUI lo pone falso para Explorer y campos modales.
Alt+Up/Down en Find/Replace conserva su significado de historial de consultas.
Ctrl+D en esos controles no duplica silenciosamente una línea del documento.

La paleta conserva el contexto del invocador y metadata del destino: DocumentId,
revisión, encoding, ViewId, pane, caret y anchor. Aceptar revalida el estado actual;
un destino cambiado se deshabilita. El menú verifica disponibilidad antes de
cerrar un modal. Un cambio válido publica estado modified/undo y actualiza las
vistas mediante la ruta de aplicación existente, sin escribir desde paint.

El mapping de mensajes GUI añade VK_OEM_2 para los atajos de `/`. La evidencia de
WM_KEYDOWN no certifica todos los layouts físicos de teclado; los menús/paleta
son rutas independientes y los checks manuales siguen al final.

No se reescriben las transformaciones de Workbench ni sus asignaciones al preparar
líneas/selecciones afectadas. Este bloque no mide ni garantiza RAM privada, coste
frío o latencia física. Continúan los contratos y límites globales F1/F2/F3.

## Validación local

Construcción y cableado integral antes de probar. Se reutilizó un worker existente
para dos archivos exclusivos de recorridos GUI/TUI; un integrador para API, State,
registro, menús, scripts, validación y Git. Rust1.85.1 MSVC x64/offline:

- **23 casos dirigidos aprobados** a la primera:15 de interacción/registro, tres
  regresiones TUI nuevas y cinco del parser CLI GUI. No corrida global del workspace.
- fmt, Clippy workspace/all-targets -D warnings y release workspace bins/examples
  aprobados. Sin nuevas dependencias ni cambios al motor Workbench de estas ediciones.
- **14 checks nativos aprobados**, siete nuevos de líneas/comentarios y siete de
  arranque/pintura. Sección `--section line-edit` (internamente
  `--smoke-line-edit-only`): HWND oculto propio, un WM_PAINT, cero errores; sin
  repetir las suites/recorridos cerrados de navegación o Explorer.
- Selección vs línea del caret, EOF con/sin separador, BOM y EOL mixtos; ABL/C#/
  Python/plain y cuatro encodings explícitos; block/line y capacidades ausentes,
  UN undo/redo exacto, split/pin, rebase de selección inactiva y GDI de solo lectura.
- Menú/paleta/atajos pasan por handlers reales. Find, Go to y Explorer no permiten
  editar fuente detrás; cambios de revisión/vista/encoding/caret invalidan edición
  propuesta por paleta. Fuente limpia→Ctrl+D modified→Undo limpia conserva
  DocumentId/ViewId y bytes originales de disco, sin Save.
- Seis ayudas/aliases y siete frames por cada PE aprobados. Fuente propia13B y
  hashes de `.git` sintético intactos; ese guard no revalida los proveedores Git.

[Logs y hashes de fuentes/binarios](evidence/m2b-line-edit-2026-11-03-metadata.json),
[reporte14](evidence/m2b-line-edit-2026-11-03-native-report.json),
[edición compartida en split](evidence/m2b-line-edit-2026-11-03-native-line-edits.png),
[fuente limpia restaurada](evidence/m2b-line-edit-2026-11-03-native-frame.png).
PNG1000×650 convertidos sin pérdida de BMP GDI y revisados visualmente.

ZIP local `work/packages/CIDE-windows-dev-line-edit-2026-11-03.zip`:3 891 607 bytes,
69 miembros, CRC y hashes del manifiesto aprobados.
[SHA-256 e inventario](evidence/m2b-line-edit-2026-11-03-package.json).
Es desarrollo local, sin instalador, publicación ni certificación de distribución.

No se ejecutó AVM ni se certifica sintaxis ABL/OpenEdge. M2A/M2B, pantalla física,
clipboard OS, IME/DPI/accesibilidad, Linux y RAM global siguen abiertos. Sin
instalaciones, perfiles cerrados repetidos, CI, push o publicación; no se pidieron
pruebas manuales al propietario. El symlink opcional omitido de otro bloque y los
dos notices GNU pendientes del paquete no se consideran resueltos por este cierre.
