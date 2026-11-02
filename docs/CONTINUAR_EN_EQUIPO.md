# Continuación local — después de optimizar S0 Unicode

F1/F2/F3 están implementados con evidencia local en `m2a/dual-pane-tui`. F1:
`ddc9d41`; F2: `2423134` + normalización `e72856f`; consultar el log para F3.
Guardado Windows: `ed51143`; S1 `6e07b1e`; S0 Unicode cooperativo implementado después (consultar log). No sustituir el árbol por main ni repetir checkpoints.

## Inicio

1. Revisar git status/rama/log y conservar modificaciones del usuario.
2. Leer AGENTS, README, PLAN, ARQUITECTURA, DECISIONES, los checkpoints F1/F2/F3,
   S0_DOCUMENT_SEARCH, S1_PROJECT_SEARCH, SAVE_WINDOWS_CONTENTION y VALIDACION, bajo `docs/`. Mediciones crudas en `docs/benchmarks/`.
3. Revisar `jc-app/src/jobs.rs`, `project_search.rs`, `project_search/scan.rs`,
   `project_search/replace.rs`, `workbench/text.rs`, `panes.rs`,
   `jc-tui/src/app/presentation.rs`, `render/mod.rs`, `frame_encoder.rs`,
   `event_loop.rs` y `terminal.rs` dentro de sus crates.
4. Validar localmente mediante `scripts/cargo-local.ps1`; no ejecutar Actions
   (M5). Tests de archivo requieren hard links/ReplaceFileW y usan temporales
   del proyecto. No descargar dependencias nuevas sin necesidad concreta.

## Contratos existentes

Un documento/sesión/undo y estado persistente único por vista. Transacciones
atómicas por revisión con mapa completo. Core byte-preserving, lectura indexada
por segmentos/rangos, proyección por revisión/encoding, EOL y checkpoints
incrementales. Layouts 1 MiB/documento, historial lógico 8 MiB/1024 pasos y
recolección conservando los undos retenidos. Cerrados conservan descriptores.

Preparación mutable antes de `Presentation` prestada; composición de solo lectura
y encoder de diferencias con baseline válido. No reintroducir reveal ni consultas
textuales que muten cachés dentro del renderer. Entrada y resultados despiertan la
UI con Wakeup; drenar lotes acotados y comprobar backlog antes de dormir.

JobSystem usa snapshots propios del rango, revisión/encoding/vista y generaciones.
Cuotas cubren pendientes, ejecución y completados sin entregar. Cancelación
cooperativa, no preempción de proveedores que ignoren el token. Workers no reciben
Workbench ni escriben buffers/disco. Propuestas requieren futura aceptación y
`apply_text_edits` por revisión; no aplicar desde poll_tasks. Configuración distinta
de encoding deberá cancelar/reemplazar explícitamente la clave. El sistema actual
está ligado a un Workbench y documentos, no a trabajos de proyecto sin documento.

## Siguiente bloque: ampliar evidencia y aceptación M2A

- Construir bloques más amplios antes de validar, según pidió el usuario. Reservar
  comprobaciones intermedias para bloqueantes concretos; no repetir suites por retoques.
- S0 está implementado: rangos cooperativos, Unicode 17 full folding, palabra,
  selección, historial, marcadores y Go to. Un solo matcher sirve documento/proyecto;
  no restaurar el escaneo síncrono, ASCII case-insensitive ni KMP paralelo.
- Decoder/scratch ya optimizados: `S0_DECODER_OPTIMIZATION.md`. Un iterador
  prestado sirve también a decode/decode_fragment; buffer reutilizado y rechazo
  ASCII sin prefijo dentro del mismo matcher. Proyecto p95 18,649 ms frente a
  40,930 ms contemporáneos y 27,457 ms S1 histórico en el mismo corpus. No extrapolar
  esa mejora a todos los casos ni afirmar mejora de cancelación. Conservar cuotas.
- Ampliar corpus Unicode/legacy, líneas extremas/frío, muchas piezas/documentos y
  teclado. Validar al cerrar ese bloque. Regex solo con motor real y presupuesto;
  no existe todavía, no anunciar cierre completo de Search Plus/M2A.
- Mantener preview/confirmación/huellas de S1 y transacciones. Abiertos no se guardan
  automáticamente; cerrados usan FileSession. Workers no escriben.
- Guardado Windows conserva retries 32/33/1175 y recovery 1176/1177; no borrar el
  temporal de recuperación ni cambiar por delete+rename. Causa histórica no atribuida.
- Prototipo GUI según PLAN, compartiendo stores/comandos; medir backends antes de
  elegir. Sublime Text es referencia visual y ecode referencia operativa. Progress M3.

## Medición y límites

El bloque de optimización pasó la suite completa de 191 pruebas Windows, fmt,
Clippy y release, y ambos flujos ConPTY (ver VALIDACION y S0_DECODER_OPTIMIZATION). Benchmarks opt-in:
`document_search_bench`, `project_search_bench`, `f3_pipeline`, `f3_jobs`; smoke Windows `scripts/smoke-conpty.ps1 -ProjectSearch`, tras compilar
`f3_console_host` y `jc-ide-tui` en release. Método en S0_DOCUMENT_SEARCH, S1_PROJECT_SEARCH y F3_PRESENTATION_TASKS.

ConPTY registra inyección-a-VT (p95 16,0726 ms en fixture mínimo), no pantalla
física. Un muestreo corto de memoria/CPU de ese proceso no certifica metas globales.
Mantener abiertas latencia física, arranque, RAM/picos con varios documentos,
servicios y doble grupo nativo. El código 1175 ya tiene reproducción nativa observada e inyección determinista;
el tratamiento seguro se probó junto con bloqueos persistentes y recovery. No
confundirlo con eliminación de toda contención externa.
F2 ya midió rangos/modelo hasta 16 MiB; no confundir
esas llamadas con latencia UI. Revisar líneas extremas, metadatos O(L), muchas
piezas y ancho Unicode/grafemas/IME/DPI. M2A continúa abierto.

Conservar bytes, BOM/EOL/CRLF, encoding explícito, guardado seguro, revisiones y
vistas independientes. No tocar Qwen, GPU, drivers, startup ni servicios ajenos.
No hacer push ni operaciones remotas sin autorización. Git con escritura y CI en
M5; plugins externos post-M5. Entregar evidencia y commit local sin reescribir
historial.

S0 optimizado medido en 16 MiB: programar p95 3 µs, slot p95 180 µs y fin p95 178829 µs;
son tiempos de modelo, no pantalla física. PowerShell tuvo bloqueo de inicialización
en esta sesión: se validó con Cargo directo en el mismo entorno de proceso y el host
C# existente compilado con csc. No se modificó configuración del sistema. El wrapper
normal sigue disponible; no investigar/tocar servicios ajenos para validar el editor.
