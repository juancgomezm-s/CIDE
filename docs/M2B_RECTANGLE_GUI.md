# Rectángulos GUI y cuadrícula compartida — 2026-11-03

**Seguimiento Unicode:** presentación/caret/hit testing ahora consumen VisualAtom
compartido, sin ruta escalar paralela en paint.17 checks HWND propios finales
(dos nuevos) aprobados; el resto de la evidencia fechada abajo es histórico.
[Contrato actual](M2_UNICODE_LAYOUT.md), sin aceptación física de GDI/IME/DPI.


Bloque posterior a `1de086a`, sobre el [contrato acotado existente](M2_RECTANGULAR_SELECTION.md).
La GUI completa Start/Cancel/Left/Right/Up/Down, menú Edit, paleta, Alt+Shift+flechas
y Alt+arrastrar. Edición, clipboard interno, presupuestos y undo pertenecen a
Workbench; no se crea otro buffer, motor o historial.

## Uso y operaciones

Start convierte extremos de selección lineal o comienza sin ancho en el caret.
Alt+Shift+flechas extiende por líneas lógicas y celdas TAB4. Alt+arrastrar empieza
un rectángulo nuevo en la vista apuntada. El primer hit convierte la presentación
escalar/diagnóstica actual a fuente y luego a coordenadas visuales; durante el drag
usa esa cuadrícula. El arrastre se limita al área visible del grupo inicial, sin
autoscroll ni creación de líneas. El teclado permite extender dentro de las cuotas.

Copy/Cut y pegado de filas usan el clipboard interno tipado existente. Una fila
se repite por altura; varias deben coincidir con ella. Pegado rectangular en un
caret usa líneas existentes y rechaza altura que sobrepasa EOF. TAB parcial se
materializa solo en el corte afectado, el resto queda en bytes originales. Texto,
Tab (cuatro espacios según indent_lines), Delete y Backspace realizan una acción
transaccional; tras una edición efectiva termina el rectángulo. Borrar espacio
virtual sin efecto no crea undo. No es multicaret persistente.

Cancel en Edit/paleta limpia solo rectángulo. Escape también cancela búsqueda
documental pendiente. Movimiento normal vuelve a selección lineal, incluso
Izquierda/Derecha si no existe byte anterior/siguiente en BOF/EOF. Click normal
convierte la cuadrícula a byte/posición escalar y sale del rectángulo; virtual
apunta al final del contenido, sin entrar en CRLF ni antes del BOM.

El [layout Unicode compartido](M2_UNICODE_LAYOUT.md) sustituyó posteriormente
la whitelist por grafemas narrow/TAB4, incluidos combinantes con base, CJK y
emoji. Controles, decodificación inválida, ancho cero y clusters >1024B se rechazan. Se valida la
línea afectada completa, no solo el corte. Hasta1024 filas/4096 columnas,64KiB por
línea,2MiB fuente/clipboard/staging por sus presupuestos existentes; no RAM privada.
Encoding estricto, BOM, EOL y newline final se conservan fuera de los cambios pedidos.

## Presentación, foco y propiedad

GUI y TUI comparten cuadrícula visual por grafemas narrow/TAB4. Con invisibles
ON, TAB aparece como `>` y relleno `.`, espacios/virtual como `.`; los clusters
se emiten completos con su ancho, incluidos glifos de dos celdas. Las marcas virtuales son presentación, no bytes copiables fuera del
rectángulo. Encabezado GUI `Rect TAB4`. La preferencia global de invisibles no cambia;
al cancelar vuelve la proyección diagnóstica con escapes. Caret, scroll, selección
y mouse usan columnas visuales de esa cuadrícula, no el ancho de un escape.

`VisualPosition` distingue línea/VisualColumn de byte y TextPosition. Entrada
absoluta `set_rectangle` y extensiones comparten admisión/LineLayout, reutilizando
filas ya validadas en la revisión actual. No lista persistente de rangos ni
decodificación del documento entero. Preparar actualiza viewport/caches visibles;
draw consume filas de solo lectura y no acepta/admite trabajo de fondo.

Cada vista posee su geometría. Split/pin conservan selección independiente y
documento/undo compartidos. Ediciones/undo/redo/encoding invalidan geometría según
el contrato F1. La paleta incluye intención visual anchor/head/revisión en su
target: moverse dentro del mismo TAB puede cambiar selección sin cambiar bytes.
TUI y GUI calculan editor_input; modales/Explorer no permiten extender o editar
fuente oculta. Copy/Cut/Paste/SelectAll/Undo/Redo también exigen destino Editor.
La limpieza del fixture restaura el clipboard interno mediante copia lineal;
la política real conserva prioridad del clipboard rectangular hasta otra copia.

## Captura y terminación

El drag conserva metadata de documento/revisión/encoding/ViewId/pane y última
intención, con anchor visual. Cada movimiento revalida; cambiar destino termina
el gesto sin tocar una selección nueva. MouseUp conserva el último rectángulo.
Capture loss, CancelMode, pérdida de foco o resize terminan drag y conservan esa
selección. Escape la cancela. Un destino no admitido deja intacta la geometría
anterior y muestra error. Las teclas de intención terminan drag antes de operar.

SetCapture/ReleaseCapture se ejecutan sin préstamo del modelo: ReleaseCapture
puede enviar WM_CAPTURECHANGED incluso cuando la llama la aplicación. No se
recaptura dentro de ese mensaje. Solo se libera captura si GetCapture es nuestro
HWND. MouseMove sin drag no provoca repintado continuo.
[SetCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture),
[ReleaseCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-releasecapture),
[WM_CAPTURECHANGED](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-capturechanged).
Los mensajes/API probados no certifican un arrastre físico, layout de teclado o DPI.

## Evidencia local

Todo se construyó e integró antes de checks. Worker existente en paint GUI y dos
archivos exclusivos de recorridos; integrador en API/State/rutas/TUI/manifest/Git.
Rust1.85.1 MSVC x64, offline:

- **26 casos dirigidos distintos aprobados:**16 del modelo afectado, cuatro TUI
  nuevos, uno de presentación invisible afectada y cinco parser CLI GUI. El caso
  core de límites de movimiento se amplió y repitió, sin sumarlo otra vez.
- **15 checks HWND aprobados**, ocho nuevos y siete arranque/pintura, sección
  rectangle: cuatro encodings, BOM/EOL, edición/clipboard interno/undo, altura/EOF/
  no-op, grid/invisibles/resize, foco/paleta, split/pin/revisión/encoding, repertorio/
  cuotas, mouse capture/Up/CancelMode/Escape y rechazo de drag obsoleto.
- fmt, Clippy workspace/all-targets -D warnings y release workspace bins/examples
  aprobados. Seis ayudas/aliases e iconos siete frames/PE aprobados; un WM_PAINT,
  cero errores. Source13B y metadataGit sintética intactos, sin Save ni OS clipboard.

Primer TUI encontró editor_input aún activo detrás de Find: se corrigió el contexto
y repitió solo ese flujo. Primer native encontró Derecha en EOF virtual conservando
rectángulo: se corrigieron ambos límites y se revalidó caso afectado +sección GUI.
Se retienen además error de compilación por shadow de owner y lint de precedencia
en empaquetado de coordenadas del fixture, corregidos antes del release final.
No se repitieron suites cerradas ni perfiles por documentación.

[Logs/fuentes/binarios](evidence/m2b-rectangle-2026-11-03-metadata.json),
[reporte15](evidence/m2b-rectangle-2026-11-03-native-report.json),
[fallo nativo inicial](evidence/m2b-rectangle-2026-11-03-native-initial-failure.json),
[cuadrícula rectangular](evidence/m2b-rectangle-2026-11-03-native-rectangle.png),
[fuente restaurada](evidence/m2b-rectangle-2026-11-03-native-frame.png).
PNG1000×650 GDI→PNG sin pérdida revisados visualmente. No corrida global del workspace,
benchmark nuevo, parser, AVM ni aceptación física/global RAM/display. M2A/M2B,
clipboard OS/IME/DPI/accesibilidad, Linux, symlink omitido y notices GNU permanecen
abiertos. Sin dependencias/instalaciones/proveedores/CI/push/publicación nuevos.

Paquete local de desarrollo `work/packages/CIDE-windows-dev-rectangle-2026-11-03.zip`:
3.973.928 bytes, SHA-256 `2600d68300c722ec2089641428f8fe0e81554ced0b0233c93e6032eac15266cc`.
CRC y hashes del manifiesto verificados para sus 69 entradas; conserva dos notices
GNU pendientes y no acredita distribución final. [Verificación](evidence/m2b-rectangle-2026-11-03-package.json).
