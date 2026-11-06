# F1 — Documentos, vistas y transacciones

**Seguimiento:** F2/F3 ya están implementados con evidencia local; consultar [rangos y memoria](F2_RANGES_INDEX_MEMORY.md) y [presentación y tareas](F3_PRESENTATION_TASKS.md). Este documento conserva el alcance/evidencia histórica de F1.

La mejora posterior de [selección histórica](M2_SELECTION_HISTORY.md) conserva
cursor/ancla/rectángulo de la vista iniciadora por transición, sin otro undo
de contenido. Sustituye la limitación histórica de restauración exacta descrita abajo.

Implementado y validado localmente el 2026-11-01 sobre la rama `m2a/dual-pane-tui`. Este incremento sustituye la sincronización de cursor entre documento y panel e introduce lotes atómicos; F2/F3 continúan pendientes.

## Propiedad real del estado

- `EditorDocument` posee `DocumentId`, asociación al catálogo `TabId`, sesión y revisión notificada. Ya no posee cursor, selección ni viewport.
- Cada grupo (`PaneView`, dentro de `PaneManager`) contiene una lista de vistas persistentes y su `ViewId` activo. Cada vista asocia una entrada del catálogo al único `ViewState` que contiene cursor, anchor y scroll. Cambiar pestaña o foco selecciona esa vista; no transfiere estado al documento.
- `DocumentId` identifica la sesión de contenido; `ViewId` identifica una presentación en un grupo; `TabId` identifica la entrada del catálogo y permite resolver su documento. Reabrir una entrada cerrada crea una nueva sesión/DocumentId. Un split crea otra ViewId, compartiendo bytes e historial.
- `active_document()` y `document_by_tab()` exponen documentos de solo lectura. `active_editor()` es una proyección prestada de sesión y estado de vista, sin copiar ninguno. Su contraparte mutable es privada del módulo Workbench. Se retiró `tabs_mut()` y la mutación pública del documento desde Workbench.
- La barra de pestañas TUI sigue mostrando el catálogo global; la selección y el estado de las vistas ya son propios de cada grupo. Cambiar el dibujo de esa barra pertenece a presentación, no exige otro buffer ni trasladar cursores.

Las colecciones son vectores pequeños dentro de Workbench/PaneManager; no se añadieron clases vacías llamadas DocumentStore/ViewStore para envolver los mismos datos. La optimización de índices, acceso a piezas y presupuestos pertenece a F2.

## Una edición lógica, un commit

`jc-core::EditBatch` incluye revisión esperada y reemplazos en coordenadas de esa revisión. El núcleo valida revisión, rangos y solapes, elimina reemplazos sin efecto y prepara piezas/geometría/reservas antes de publicar. Rechaza inserciones ambiguas en el mismo origen. Un lote efectivo publica una revisión, un estado de undo y un `ChangeSet` completo. No copia el archivo completo para simular rollback. `replace`/`insert`/`delete` usan este mismo mecanismo con un reemplazo.

`ChangeSet` contiene revisiones anterior/nueva y geometrías ordenadas para mapear posiciones secuencialmente. Undo devuelve el inverso completo; redo devuelve el cambio completo. `last_edit()` permanece como consulta de compatibilidad solo para cambios de una geometría; la aplicación consume `last_change()`.

`Workbench::apply_text_edits(DocumentId, expected_revision, Vec<TextReplacement>)` permite proponer texto para un documento abierto incluso sin enfocarlo. Comprueba encoding estricto y fronteras textuales antes de aplicar el lote; un carácter no representable o rango inválido no publica cambios. Los frontends/proveedores no necesitan obtener el buffer mutable. Find/Replace actual y múltiple usan esta entrada.

Las vistas guardadas en ambos grupos y los historiales de navegación consumen el mismo mapa. En comandos locales, el comando determina la selección final de la vista activa; las demás se reubican. En propuestas de texto y undo/redo se mapean posiciones con afinidad a la derecha, colapsando posiciones interiores al final del reemplazo; después se ajustan a fronteras textuales válidas. La política actual es determinista, **no conserva una fotografía exacta de las selecciones previas a cada transacción**. Si se añade esa restauración de UX, deberá guardarse como metadato acotado del historial, sin segundo undo de contenido.

CRLF es indivisible para desplazamiento/borrado textual y destinos de navegación. Las coincidencias de búsqueda también exigen fronteras textuales. Los bytes internos siguen editables desde el core para sus consumidores de bytes; no se normaliza el documento. La normalización de posiciones de todas las vistas evita dejar un caret interno cuando una edición une CR y LF previamente separados.

## Apertura, cierre y modificación

Cerrar una vista cuyo documento conserva otras referencias de vista no pide descartar el documento ni borra su undo. Cerrar la última requiere la decisión de cambios pendientes. La reapertura conserva solo ruta, encoding, identidad del catálogo y condición de archivo nuevo: un archivo existente se relee de disco; un borrador descartado se reabre vacío. Si un archivo existente no se puede leer, la reapertura falla conservando el descriptor para reintentar. No hay recuperación implícita de texto descartado.

Las decisiones de guardar/cerrar/contexto consultan `Document::is_modified()`, derivado de la revisión guardada. El booleano de TabManager es una proyección para presentación, actualizada por Workbench; no decide la integridad del contenido. Las sesiones cerradas ya no retienen bytes/undo completos.

El prompt Open termina antes de abrir la siguiente elección de encoding. Se comprobó con fuente legacy que la elección permanece visible hasta seleccionar su code page; los bytes quedan intactos. FileReopenClosed vuelve a solicitarla si la sesión cerrada carecía de encoding explícito.

## Evidencia local

Windows, Rust 1.85.1 MSVC, wrapper `scripts/cargo-local.ps1`, dependencias offline. Pruebas nuevas en `jc-core/tests/transactions.rs`, `jc-app/tests/f1.rs` y eventos TUI. Cubren lotes multirrango/inversos, error y redo intactos, comparación con historial plano, replace-all en un undo, vistas independientes por grupo/documento, ediciones de proveedor sin cambiar foco, CRLF, UTF-8/BOM, legacy estricto y descarte/reapertura. La prueba anterior de cierre global fue actualizada para exigir que sobreviva la otra vista.

Resultados y comandos finales en [VALIDACION.md](VALIDACION.md). No hay aceptación de rendimiento ni nueva matriz manual de terminales por este incremento. F2 todavía debe eliminar copias/decodificación completas, limitar memoria por bytes y comparar estructuras; F3 separará render de actualización y añadirá jobs acotados/versionados. La GUI y el estilo visual de Sublime continúan según [PLAN.md](PLAN.md).
