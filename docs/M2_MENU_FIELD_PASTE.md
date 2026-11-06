# M2 — Pegar desde menús conserva el campo — 2026-11-06

El menú y Ctrl+V deben operar sobre el mismo campo visible. La revisión paralela
GUI/TUI encontró dos desajustes reales en ese contrato, sin requerir escritorio.

## Corrección integrada

GUI: Editar → Pegar sobre la consulta Git cerraba el espacio Git antes de resolver
la acción. El despacho podía terminar en el editor. En plantillas se cambiaba la
generación del contexto innecesariamente. Buscar/Reemplazar, Ir a posición y los
borradores de proyecto admitían Ctrl+V pero carecían de intención equivalente desde
el menú. field_paste_action captura ahora el destino editable visible, sin leer el
portapapeles; disponibilidad, ejecución y menú usan esa misma decisión. Se conservan
las acciones tipadas y sus comprobaciones de generación, revisión y destino.
El menú conserva la superficie cuando hay un campo real. Selectores mantienen su
ruta propia de selección/cortar/copiar/pegar. Las vistas previas y resultados de
solo lectura no habilitan un pegado hacia el documento que queda detrás.

TUI: el menú calculaba Pegar desde el overlay Menu, ocultando el presupuesto del
campo persistente Git/plantilla. Ahora la disponibilidad toma ese destino subyacente;
solo al ejecutar el elemento y cerrar el menú se permite leer el transporte. Ctrl+V
y Event::Paste siguen sin atravesar el menú. La preparación del menú no lee datos.

No cambia el diseño, tema oscuro ni textos en español; no se añaden servicios,
dependencias, escrituras Git ni otro modelo de edición. El núcleo sigue independiente.

## Validación del bloque

20 casos dirigidos distintos pasaron por suites: 19 con filtro paste de GUI/TUI y
uno adicional del reemplazo de proyecto. Ocho son nuevos (seis GUI, dos TUI).
Comprueban la equivalencia menú/teclado y conservación de campo, foco, generaciones,
borradores, bytes y vistas; rechazan pegar en previews/resultados y no inician
búsquedas por pegar una consulta. TUI usa transporte en memoria y comprueba un clic
en Pegar sobre plantilla y Intro sobre consulta Git. GUI verifica las intenciones
previas a transporte; no afirma transferencia real del portapapeles del sistema.

Formato, Clippy GUI/TUI all-targets y release workspace offline se registran en
[evidencia](evidence/m2-menu-field-paste-2026-11-06.json). Se reutilizan las cachés
embedded-git-next para pruebas/revisión y build-cache para release. No se repiten
las suites completas, HWND, ConPTY ni pruebas físicas. No se toca el escritorio,
portapapeles o recuperación personal ni se activa CI o push.

Entrega: target/m2-field-paste-next/release, manteniendo las anteriores. El estado
vigente de M2 distingue este cierre funcional de aceptación física/global:
[Matriz de cierre](M2_CIERRE.md). IME excluido, Narrador aplazado y segundo monitor
sin verificar. Ninguna prueba de este bloque sustituye esas comprobaciones.
