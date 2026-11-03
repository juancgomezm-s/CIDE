# Snippets y Surround With — TUI implementada, 2026-11-03

Construido sobre31649b5: motor, catálogo opcional, TUI y recorridos validados
localmente al cerrar el bloque. Sin dependencias externas nuevas.

## Motor y datos

Motor universal en jc-app; plantillas son contribuciones declarativas del
proveedor opcional. progress.abl aporta Procedure/Function/Trigger y envoltorios
IF/DO/REPEAT/FOR EACH comunes, sin casos ABL en el host. No son scripts ejecutables
ni plantilla certificada en una versión de OpenEdge. No AVM, semántica o OO inferido.

Catálogo por perfil/ID namespaced, hasta16 plantillas por proveedor, cuerpos
estáticos de hasta8KiB y hasta cuatro parámetros de256 UTF8. Variables acotadas:
parámetros declarados, selection y cursor; sin evaluación ni expansión recursiva.
Los nombres/valores se validan según metadata genérica, nunca una rama por lenguaje.

SnippetSession captura DocumentId/revisión/encoding/ruta/ViewId/caret/anchor y
rango lineal legal; no Document/FileSession/undo ni fuente completa. Selección
de hasta16KiB fuente, decode estricto y propuesta de hasta64KiB UTF8. Rectángulos
y selección no interpretable se rechazan antes de editar. Un BOM UTF8 inicial
queda fuera del rango reemplazable; BOM interior permanece literal.

Las LF del cuerpo estático usan el EOL preferido del documento. El contenido
seleccionado conserva sus EOL exactos; Surround mueve un EOL final seleccionado
al final del envoltorio para conservar el salto terminal. Parámetros son texto
de una línea, sin normalización ni interpolación secundaria. Ningún marcador de
caret puede terminar dentro de CRLF, incluso al juntar propuesta y contexto.

## TUI y transacción

Edit/Palette Insert Snippet y Surround With: elegir plantilla, completar parámetros,
revisar original/propuesta por páginas, confirmar o cancelar. El campo se edita
con la mecánica genérica de FieldEdit; el texto pegado en el wizard nunca edita
el documento. Menú/paleta proporcionan fallback de teclado; foco/edición fuente
permanecen suspendidos mientras está abierto.

Aceptar exige el mismo origen, selección y caret y publica un TextReplacement
por revisión esperada con una transacción. Encoding estricto antes de commit;
sin guardar ni writes desde proveedor. Undo/redo y las otras vistas usan el mapa
F1. Preview es inmutable/acotado y render de solo lectura, sin reexpandir el motor.
Cambiar contexto invalida la aceptación; no se reaplica una propuesta antigua.

## Salida de bloque

Construir antes de pruebas. Validación dirigida de parámetros/variables/cuotas,
UTF8/legacy/BOM/mixedEOL/newline final, cursor legal, selección/rectángulo,
revisión/caret/vista/ruta stale, cancelación/preview/undo y eventos TUI/ConPTY.
Luego inventario final de teclado y puertas funcionales TUI, conservando los
pendientes físicos/globales y de proveedores reales que requieran configuración.

## Cierre observado

38 casos Rust distintos aprobaron (11 nuevos): siete del motor, siete de registro,
cuatro TUI, cinco del proveedor y15 de interacción. Save As requirió acceso al
temporal propio fuera del sandbox; pasó con ese acceso autorizado. Se conservan
logs iniciales, correcciones de fixtures/lint y cierre dirigido. La declaración
insertada termina en EOL para no pegar END al fuente posterior. Fmt, Clippy
workspace/all-targets y build release workspace bins/examples aprobaron offline.

Cuatro flujos ConPTY reales aprobaron: preview/cancelación/tecleo aislado,
resize sin aplicar, aplicación+undo/redo único y Surround selección+undo.
47306 bytes VT, salida0, fixture original intacto y ningún AVM iniciado.
Enter/Tab avanza; Shift+Tab vuelve; F2 edita desde preview; F9/Ctrl+Enter acepta;
Escape cancela. Las plantillas son texto común, no compiladas en OpenEdge.
No nueva validación GUI, clipboard OS, pantalla física ni rendimiento global.

[Metadatos/hashes](evidence/m2-snippets-2026-11-03-metadata.json), [ConPTY](evidence/m2-snippets-2026-11-03-conpty-1.json). ZIP de desarrollo `work/packages/CIDE-windows-dev-snippets-2026-11-03.zip`, SHA256 `5741a638e8d099f57284275300baa8a7dd4956d506c75eb46ff48964dc9d6e83`; CRC/manifiesto verificados. Dos avisos GNU siguen pendientes; no certificación de distribución.
