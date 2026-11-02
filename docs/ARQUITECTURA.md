# Arquitectura

JC-IDE separa almacenamiento de bytes, interpretación textual, estado de aplicación, servicios opcionales y presentación. Esta es la arquitectura de destino; el estado implementado y sus diferencias están en la [auditoría del 2026-11-01](AUDITORIA_ECODE_ARQUITECTURA.md).

F1 ya separó documento y vistas, retiró campos espejo e introdujo transacciones multiposición y mapas completos. El [checkpoint F1](F1_DOCUMENT_VIEWS_TRANSACTIONS.md) documenta la implementación real. F2 ya incorpora rangos, índice por revisión, EOL/checkpoints incrementales y cuotas. F3 separa preparación y composición de solo lectura, emite diffs de celdas y añade snapshots propios acotados, generaciones, cancelación cooperativa y despertar por resultados. El [checkpoint F3](F3_PRESENTATION_TASKS.md) precisa cuotas y evidencia; la latencia física y RAM global siguen abiertas. El [checkpoint F2](F2_RANGES_INDEX_MEMORY.md) distingue resultados del modelo de objetivos de RAM/latencia globales.

## Capas

- **jc-core**: documento byte-preserving, piece table, revisiones, undo/redo acotado, compactación y regiones.
- **jc-text**: interpretación explícita de encoding, line endings y coordenadas sin poseer los bytes canónicos.
- **jc-files**: ciclo de vida de archivo, conflictos externos, creación segura y reemplazo atómico.
- **jc-app**: comandos, keymap contextual, foco, menús, pestañas, paneles y diálogos. No dibuja nada.
- **jc-languages**: perfiles, detección y metadatos declarativos de capacidades (por ejemplo, comentarios). No inicia servicios por reconocer una extensión.
- **jc-progress**: integración explícita con una instalación real de OpenEdge; candidata a adaptador de la extensión interna `progress.abl`.
- **jc-tui**: frontend terminal existente; traduce eventos, consulta presentación y dibuja.
- **jc-gui**: frontend gráfico pendiente; consume el mismo modelo sin poseer otro documento ni undo.

La separación entre núcleo universal, capacidades por lenguaje y extensiones internas está definida en [EXTENSION_ARCHITECTURE.md](EXTENSION_ARCHITECTURE.md).

La regla central es que una acción de negocio existe una sola vez como `CommandId`, con argumentos tipados y contexto. Un menú, una tecla, la command palette o un botón gráfico resuelven el mismo comando. Los frontends aportan intención/destino; no implementan otras reglas de guardar, abrir lateralmente, compilar, buscar o navegar. Entrada de texto, composición IME y puntero también atraviesan el servicio de edición compartido.

## Propiedad del estado

Propietarios lógicos de destino: F1 los implementa mediante colecciones en Workbench y grupos; DocumentId/ViewId ya existen. F2 implementa TextProjection y lecturas prestadas por revisión; F3 implementa snapshots propios de rangos para workers y presentación prestada de solo lectura; no son clones de documentos ni otro motor editable.

| Propietario | Contiene | No contiene |
|---|---|---|
| `DocumentStore[DocumentId]` | Bytes, revisión, historial único, `FileSession`, encoding y revisión guardada | Caret activo, scroll o foco |
| `TextIndex[DocumentId]` | Índice de líneas/rangos y cachés compartidas, identificados por revisión/encoding | Otra copia canónica editable del archivo |
| `ViewStore[ViewId]` | `DocumentId`, selecciones con afinidad, viewport, columna preferida, configuración de wrap | Otra sesión de archivo u otro undo de contenido |
| `EditorGroup[GroupId]` | Lista ordenada de vistas/pestañas, vista activa, pin e historial de navegación | Copia del cursor de cada vista |
| `Workbench` | Stores, foco, dispatcher, paneles y coordinación de cambios/tareas | Detalles de celdas, píxeles o procesos ABL obligatorios |
| Frontend | Geometría, recursos de dibujo y cachés de layout por vista | Mutaciones directas del buffer o estado de negocio paralelo |

El `TabId` actual mezcla identidad de documento y presentación. La migración lo retira o lo limita a presentación; no se añaden más significados. Dos grupos pueden mostrar el mismo `DocumentId` mediante distintos `ViewId`. Abrir una ruta ya registrada reutiliza documento/sesión. La política de identidad de archivo debe cubrir rutas relativas/canónicas, mayúsculas y alias sin fusionar borradores distintos.

Cerrar una vista elimina solo esa referencia. Cerrar la última vista verifica cambios pendientes; el indicador de modificación se deriva de la revisión guardada, no de un booleano de pestaña sincronizado a mano. La reapertura usa metadatos mínimos; recuperación de borradores, si se ofrece, tiene semántica y presupuesto propios. El historial de navegación no debe retener buffers completos accidentalmente.

## Cambios atómicos y notificación

`EditBatch` contiene `DocumentId`, revisión esperada, reemplazos en coordenadas de esa revisión e información de intención/vista iniciadora. Antes de mutar se validan encoding, límites de edición, rangos, solapes y reservas necesarias; una propuesta obsoleta se rechaza o se recalcula explícitamente. No se reinterpreta silenciosamente sobre otra versión.

Un commit publica una revisión nueva y un `ChangeSet` completo: rangos anteriores/nuevos y mapa de posiciones con afinidad en bordes. Todas las vistas, selección, navegación, índice textual, búsqueda y proveedores consumen ese mismo cambio. Undo/redo restauran una transacción lógica y notifican el mapa inverso/directo completo. El estado guardado sigue identificable a través de undo/redo; no basta contar comandos.

No exponer `document_mut()` a widgets o proveedores. Ningún consumidor puede cambiar contenido evitando la transacción. Multi-cursor, replace-all, formato y acciones de código usan el mismo camino; no se implementa atomicidad copiando todo el archivo para cada comando ni llamando a N undos desde la UI.

El historial de contenido pertenece al documento. F1 reubica las selecciones mediante mapas, con afinidad a la derecha y ajuste a límites válidos. Restaurar exactamente la selección previa de la vista iniciadora es una mejora de UX todavía pendiente; se añadirá con metadatos acotados, sin otro undo de contenido. En la política de destino, la selección que restaurará undo se define para la vista iniciadora; las demás vistas se reubican. Si esa vista ya cerró, el cambio de contenido sigue siendo válido y la política de selección debe ser determinista. Se cubre en pruebas, no mediante sincronización implícita de caret global.

## Acceso textual y memoria

El core ofrece segmentos/rangos y snapshots de lectura por revisión. Un índice aumentado de líneas permite localizar bytes/líneas sin materializar el archivo completo. `jc-text` decodifica rangos con contexto suficiente para límites multibyte, incidencias y CRLF; reutiliza cachés acotadas e invalida solo lo afectado. Apertura inicial o cambio de encoding pueden exigir recorrido global explícito; mover el cursor sobre texto estable no.

F2 comparó prefijos planos, bloques y árbol Box con la misma carga y eligió un vector de piezas con índice de bloques de 128. El árbol queda como candidato para cargas futuras de transacciones, no otro motor activo. Las mediciones y límites están en el checkpoint F2. La interfaz de cambios se fija antes de esa elección. Medir N bytes, P piezas, cantidad de líneas y una línea extrema: limitar líneas visibles no limita por sí solo el trabajo horizontal sobre una línea enorme.

Presupuestar bytes de undo/redo, segmentos añadidos, snapshots, cachés y documentos cerrados, además del número de pasos. Recuperar almacenamiento no alcanzable sin borrar undo todavía prometido. Un snapshot de worker tiene duración/cancelación acotada para no impedir indefinidamente la liberación.

## TUI y GUI

La TUI existente valida flujos y teclado. La GUI se prototipa tras estabilizar los contratos F1/F2, sin esperar a completar proveedores ABL. Sublime Text es la referencia visual; ecode aporta patrones de operación documentados en la auditoría. La compatibilidad con plugins Sublime sigue post-M5.

El ciclo común es evento → comando/edición → cambios e invalidaciones → layout visible → render de solo lectura. Dibujar no cambia foco, caret ni viewport. El frontend solicita únicamente filas/rangos visibles con margen acotado; las filas de resultados también se virtualizan. En terminal se comparan celdas y se permite invalidación completa al redimensionar; en GUI se invalidan regiones/layout afectados. No hay render continuo cuando no cambió nada.

El contrato normativo de la TUI está en [TUI_INTERACTION.md](TUI_INTERACTION.md).

## Codificación y coordenadas

El documento canónico contiene bytes. La página de códigos es metadato explícito. `jc-text` puede interpretar UTF-8, Windows-1252, ISO-8859-1 e IBM850, pero no convierte el almacenamiento. Un legacy ambiguo exige selección de proyecto/documento. El núcleo no normaliza CRLF, BOM ni salto final.

Se distinguen offsets de byte, caracteres decodificados, grafemas, columnas visuales, posiciones LSP y posiciones reportadas por compiladores mediante tipos explícitos. Las conversiones ocurren en límites documentados y usan revisión/encoding. Un CRLF es una frontera textual indivisible; el offset interno sigue siendo válido para operaciones de bytes del core, pero no para un caret textual. El ancho visual depende de tabs, grafemas, fuente y frontend; no equivale al número de escalares.

## Trabajos y proveedores

La UI coordina mutaciones en un único flujo. Workers reciben snapshots inmutables y solicitudes con `JobId`, generación y revisiones; devuelven resultados/propuestas, nunca escriben al documento vivo. Al recibirlos se verifica que documento, vista/destino y configuración sigan vigentes.

Búsqueda, exploración de archivos, Git y lenguajes usan concurrencia, colas y resultados acotados, cancelación y presupuesto. El loop de UI debe despertarse por entrada o finalización sin depender de otra tecla. Search Plus entrega lotes y da prioridad a contenido sucio abierto frente al archivo de disco. Una cancelación o cierre descarta entregas tardías y libera snapshots.

En reemplazo de proyecto la atomicidad es por documento/archivo y se informa avance/conflictos: no se promete una transacción atómica de todo el filesystem. Preview y validación de revisión/huella preceden a cada escritura. Progress/AVM solo se inicia a petición explícita; navegación estática no ejecuta programas.

La implementación F3 se limita a tareas internas de documento con vista opcional. Son dos workers lazy, ocho trabajos admitidos, snapshots de 2 MiB/8 MiB total, resultados de 64 elementos/64 KiB y deadline cooperativo de 2 s por defecto. Una cola ocupada devuelve Busy; no se fuerza la interrupción de Rust arbitrario. La entrega vuelve a validar documento/revisión/encoding/vista/generación. Configuración de proyecto, escaneo por lotes y proveedores reales aún deben integrarse; propuestas no se aplican automáticamente. Véase el checkpoint F3 para el loop y sus cuotas de entrada/logs.

El incremento S1 usa un controlador de proyecto compartido y un worker bajo demanda, además de los jobs documentales F3. Captura hasta 8 MiB de abiertos incluidos (2 MiB/archivo), conserva solo una petición pendiente y entrega lotes de hasta 32 filas con cola de cuatro, más evento terminal. Workers solo leen. La aplicación consume planes de reemplazo confirmados de uno en uno: abiertos mediante apply_text_edits por revisión; cerrados mediante FileSession, con huella y conflicto antes de guardar. El renderer solo lee preview/filas visibles. No hay segundo undo ni transacción global del filesystem. Véanse [S1](S1_PROJECT_SEARCH.md) y [contención de guardado Windows](SAVE_WINDOWS_CONTENTION.md).

## Ligereza

Abrir y editar no requiere Progress, Git, LSP, terminal ni agentes. Los servicios se activan bajo demanda y su consumo se mide junto con el proceso principal.

## Interfaz Windows

La tecnología gráfica sigue pendiente de medición. Comparar candidatos con el mismo caso y el mismo motor: arranque, memoria privada, tecla-a-pantalla, IME, accesibilidad, DPI, selección y archivos grandes. Un control existente solo es viable si puede respetar la propiedad del documento y del undo; si exige otro buffer editable/normalización obligatoria, queda descartado o requiere revisar explícitamente la decisión de arquitectura antes de integrarlo.

Tema, densidad y disposición se definen como datos. Primera escena: pestañas por grupo, sidebar colapsable, editor dominante, palette flotante, búsqueda inferior, barra de estado y referencia anclada. Minimap y efectos no preceden a los presupuestos de respuesta. Las metas y evidencia se mantienen separadas en [PLAN.md](PLAN.md) y [VALIDACION.md](VALIDACION.md).
