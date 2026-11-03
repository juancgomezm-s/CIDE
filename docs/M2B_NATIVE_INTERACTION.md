# Menús nativos, selección lineal y rueda — 2026-11-03

Bloque posterior a `ae9a813`. Integra los dos huecos universales pendientes del
inventario: adaptación de menús y gestos de ratón sobre las vistas fuente. El
documento, transacciones, encoding, deshacer y presentación siguen perteneciendo
a sus propietarios existentes. No introduce widget EDIT, motor de texto, proveedor
o dependencia nueva. Sublime Text continúa como referencia visual.

## Contrato de ratón

Click en texto coloca el caret sin crear historia de saltos. Arrastrar mantiene
el ancla y el grupo donde empezó; Shift extiende desde el ancla lineal existente
(o caret si había rectángulo). Los hits convierten explícitamente desde la
proyección vigente —escalares, diagnósticos de invisibles o TAB4 del rectángulo—
a fronteras byte del documento. Espacio después de EOL se limita a su frontera;
CRLF y escalares multibyte no se parten. El BOM inicial conserva la convención
de selección completa con ancla0, sin permitir un caret dentro de sus bytes.

El gesto capturado se limita al área visible del grupo original, sin autoscroll.
MouseUp, pérdida de captura, CancelMode, resize, pérdida de foco, tecla, escritura
o apertura de menú terminan el gesto conservando la última selección. Un cambio
de documento/revisión/encoding/ViewId/pane/caret/ancla/rectángulo/ruta/raíz/foco/
invisibles invalida la entrega tardía. Se comparte esta protección con Alt+drag
rectangular. No se añade word-selection por doble click ni drag del campo selector.

La rueda actúa sobre el viewport fuente bajo el puntero, incluido su gutter,
conservando foco, caret y selección. Acumula fracciones de120 por destino; cambiar
el contexto descarta el resto anterior. Respeta la cantidad de líneas configurada
por Windows, cero desactiva y page-scroll usa las filas visibles; el valor de
líneas se limita a128. Ctrl/Shift/Alt+wheel no inicia zoom ni scroll horizontal.
Paneles y modales no desplazan la fuente oculta. La rueda termina cualquier drag.

`manual_scroll` vive una vez en cada ViewState. Suspende el reveal automático
hasta un movimiento/selección/edición/salto explícito; un resize limita el viewport
sin mover caret. El enlace vertical aplica el delta realmente consumido por el
grupo, limitado por el otro documento; pin es independiente. No hay copia de
documento, nuevo historial, decode completo ni índice permanente de ratón.

La conversión screen→client de WM_MOUSEWHEEL y lectura de configuración se hacen
fuera del préstamo del modelo. SetCapture/ReleaseCapture también quedan fuera:
la liberación puede reentrar mediante WM_CAPTURECHANGED. Solo se libera captura
si pertenece al HWND propio; un fallo conserva la selección y termina el gesto.

## Contrato de menú

File/Edit/Search/Project/Tools/View nacen del modelo compartido y de handlers GUI
reales. Navigation y Git conservan las familias implementadas. Cada acción de
negocio aparece una sola vez, incluidos Open File to Side, sangría, rectángulo,
tabs y foco; separadores vacíos/dobles se eliminan. Build/Help no reciben contenido
inventado. Se elimina la clasificación anterior por rangos numéricos.

Alt+F/E/S/P/T/V/N/G y F10 abren el grupo correspondiente incluso desde un modal.
Las seis macros MenuFile/Edit/Search/Project/Tools/View de paleta llegan al HMENU
real. Los comandos de negocio siguen entrando por su handler existente y su
capability vigente. Las etiquetas de atajo se resuelven para el control activo;
por ejemplo Alt+1 en Bottom representa Problems y no se anuncia como Tab1.
Los checks reflejan invisibles, enlace, pin, paneles y flags de búsqueda.

WM_INITMENU captura fuente, campo selector, foco, raíz y generaciones modales.
La elección revalida ese origen y su capability antes de cerrar un modal o
ejecutar. Caret/query/candidato/encoding/ruta/raíz/revisión/vista cambiados durante
la llamada descartan la elección. Cancelar no ejecuta. El transporte nativo no
mantiene préstamos State/Workbench; se revalida además HWND/HMENU/grupo e identidad
del comando al volver. La marca acotada no posee un documento ni su contenido.

Son94 rutas de los104 IDs del inventario histórico:88 acciones de negocio y seis
macros de menú ahora conectadas. El inventario original y sus hashes son evidencia
histórica; este incremento no certifica104 funciones ni todos sus caminos físicos.

Referencias de ABI: [WM_MOUSEWHEEL](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel),
[captura de ratón](https://learn.microsoft.com/en-us/windows/win32/learnwin32/other-mouse-operations),
[menús nativos](https://learn.microsoft.com/en-us/windows/win32/menurc/using-menus).

## Evidencia local

Todo el cableado de APIs/State/rutas/hits/captura/foco/render/menús y el arnés quedó
construido antes de ejecutar pruebas. Un worker existente construyó archivos
exclusivos de menú y después arnés; el integrador reunió el bloque y ejecutó la
validación serial. **16 casos Rust distintos aprobados**, cuatro nuevos: viewport
y selección byte(1), vistas compartidas afectadas(9), definiciones/macros de menú
(2), parser CLI(1), acceso acotado de viewport F2(1), composición y split TUI(2).
No se repitieron suites cerradas de negocio ni pruebas globales sin impacto.

**13 checks HWND aprobados a la primera**, seis nuevos y siete de arranque/pintura.
Incluyen cuatro encodings, BOM, EOL mixto, multibyte/CRLF indivisibles, selección
por diagnósticos, reemplazo con un undo, Shift y captura/interrupción/resize.
Seis cambios de intención invalidan drag; rueda signed ±60/120/240 sobre grupo
inactivo conserva foco/caret, acumula fracciones, respeta enlace/pin y termina
ambos gestos. Preparación conserva scroll manual y movimiento recupera reveal.

Se inspeccionó el HMENU real: ocho grupos, IDs y separadores frente al modelo;
ocho Alt, F10, seis macros de paleta, Tools→Palette y Edit→Indent. Estados de
fuente/selector/Find/Bottom y labels contextuales; Copy/Cut/Paste solo con transporte
sintético. Seis reentradas de fuente y tres de campo descartan elecciones tardías;
wrong-group/disabled rechazados. Find→menú cancelado conserva su draft; no se
ejecutó un caso separado de paleta→macro de menú con Find subyacente.

Resize real propio/compact, hits, preparación y raster GDI conservan modelo,
carets/scroll y contadores de trabajos/delivery. Source13B conserva bytes, encoding,
DocumentId/revisión, catálogo y disco; metadata Git propia intacta. Un WM_PAINT,
cero errores. Son pruebas de transporte y composición; la elección de popup se
inyecta y no prueba TrackPopupMenuEx con ratón/teclado físicos.

fmt, Clippy workspace/all-targets `-D warnings` y release workspace bins/examples
aprobados, Rust1.85.1 MSVC x64 offline. Se conserva el fallo inicial de Clippy:
tres lints solo del arnés (precedencia, format_collect y comparación con vacío).
Se corrigieron juntos; ningún caso Rust/native falló. No se midió RAM privada ni
latencia global; los24ms del arnés no son latencia física de interacción.

[Logs, hashes y nombres distintos](evidence/m2b-native-interaction-2026-11-03-metadata.json),
[reporte HWND](evidence/m2b-native-interaction-2026-11-03-native-report.json),
[scroll enlazado](evidence/m2b-native-interaction-2026-11-03-native-interaction.png),
[fuente restaurada](evidence/m2b-native-interaction-2026-11-03-native-frame.png).
PNG1384×741 y984×641 revisados visualmente, conversión BMP→PNG sin pérdida.
El raster solo contiene el área cliente: no es captura visual del menú desplegado.

No se afirma aceptación física, clipboard OS, IME/DPI/accesibilidad, Unicode
general, Linux, RAM o latencia global. Permanecen pendientes los proveedores
internos y sus puertas, mouse horizontal/autoscroll/word-selection/campos,
symlink opcional y dos notices GNU. Sin dependencias/proveedores/AVM/regex/
instalaciones/seguridad nuevos, CI/push/publicación. GitHub Actions dormido hastaM5.

Paquete `work/packages/CIDE-windows-dev-native-interaction-2026-11-03.zip`:
4.177.463 bytes, SHA-256 `f59348b8a6cf0940bc1243b5e54aa653ad6404bfd6e6b95a2fd9edd7f63bca52`.
CRC y hashes de69 miembros aprobados; [detalle](evidence/m2b-native-interaction-2026-11-03-package.json).
Dos notices GNU pendientes; es un ZIP de desarrollo local.
