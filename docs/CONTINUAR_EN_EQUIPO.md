# Continuación local — F3 de JC-IDE

F1/F2 están implementados en la rama local `m2a/dual-pane-tui`; F2 parte de `ddc9d41`. Consultar el log para su commit final. No sustituir el árbol por main ni repetir los checkpoints anteriores.

## Inicio

1. Revisar git status/rama/log y conservar modificaciones del usuario.
2. Leer AGENTS, README, PLAN, ARQUITECTURA, DECISIONES, F1_DOCUMENT_VIEWS_TRANSACTIONS, F2_RANGES_INDEX_MEMORY y VALIDACION (documentos bajo `docs/`). Las métricas reproducibles están en `docs/benchmarks/` y los ejemplos opt-in `audit_baseline`, `f2_text` y `range_layouts`.
3. Revisar `jc-app/workbench/text.rs`, `jc-core/document.rs`, `jc-app/panes.rs`, `jc-tui/render/mod.rs`, `jc-tui/terminal.rs` y su loop `lib.rs`, bajo `crates/*/src/`.
4. Validar localmente lo que cambie con `scripts/cargo-local.ps1`; no ejecutar GitHub Actions (M5). Las pruebas de archivos requieren hard links/ReplaceFileW y usan temporales del proyecto.

## Contratos que ya existen

Un documento/sesión/undo, vistas persistentes con selección/viewport únicos y transacciones atómicas por revisión. Core byte-preserving con lectura por segmentos/indexada en bloques; proyección de texto por revisión/encoding con índices EOL y checkpoints escalares actualizados localmente. Layouts con cuota de 1 MiB/documento e historial lógico de 8 MiB/1024 pasos. Recolección recupera fuentes muertas sin borrar undo conservado. Documento cerrado retiene descriptor, no buffer.

La TUI ya lee filas/columnas visibles y virtualiza resultados; sus menús, keymap y comandos son compartidos. Mantenerlo al evolucionar GUI: dirección visual de Sublime Text, ecode como referencia de operación.

## Siguiente incremento: F3

- Separar actualización y render: reveal/viewport ocurren antes de crear presentación; dibujar consume lectura y no muta Workbench.
- Emitir diferencias entre pantallas/celdas; invalidación total solo cuando corresponde (resize o recuperación), sin perder estilos/cursor ni semántica de overlays.
- Definir jobs/resultados con identidad, generación, revisión y cancelación; workers entregan propuestas, no mutaciones. Concurrencia, colas, lotes y resultados tienen cuotas explícitas. La UI despierta por resultados sin esperar otra tecla ni girar en busy loop.
- Diseñar snapshots propios de lectura para jobs con duración/cuota acotadas. `DocumentRead` es un préstamo estable; no sirve por sí solo para mantener un worker leyendo mientras la UI muta. Evitar clon de archivo completo por tecla y otro motor de edición.
- Validar primero con proveedor de prueba: resultados obsoletos, cierre de documento/vista, cancelación y presión de cola. Progress real sigue M3, Git con escritura/CI M5 y plugins Sublime/Python post-M5.

## Medición y límites

F2 pasó 142 pruebas locales y comparó estructuras/cargas hasta 16 MiB, incluyendo Unicode en una línea. Los datos son del modelo, no RAM privada ni latencia UI. F3 medirá tecla-a-pantalla, reposo, cuotas y cancelación, conservando los benchmarks como regresión.

Pendiente perfilar picos fríos de layouts de líneas extremas, desplazar metadatos O(L), fragmentación/transacciones con muchas piezas y múltiples documentos. Search de documento aún escanea síncronamente al reconstruir consultas. Mantener explícitos esos límites; no certificar <50 MiB ni 16 ms de UI a partir de llamadas del modelo.

Conservar bytes, encodings explícitos, BOM/EOL/CRLF, guardado seguro, revisiones, un historial undo por documento y vistas independientes. No tocar Qwen, GPU, drivers, startup ni servicios ajenos. No hacer push ni operaciones remotas sin autorización del usuario. Entregar evidencia local, documentación y commit local sin reescribir historial.
