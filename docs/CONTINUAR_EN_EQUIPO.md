# Continuación local — después de Search Plus literal

F1/F2/F3 están implementados con evidencia local en `m2a/dual-pane-tui`. F1:
`ddc9d41`; F2: `2423134` + normalización `e72856f`; consultar el log para F3.
Guardado Windows: `ed51143`; Search Plus literal de proyecto y reemplazo con preview implementados después (consultar log). No sustituir el árbol por main ni repetir checkpoints.

## Inicio

1. Revisar git status/rama/log y conservar modificaciones del usuario.
2. Leer AGENTS, README, PLAN, ARQUITECTURA, DECISIONES, los checkpoints F1/F2/F3,
   S1_PROJECT_SEARCH, SAVE_WINDOWS_CONTENTION y VALIDACION, bajo `docs/`. Mediciones crudas en `docs/benchmarks/`.
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

## Siguiente incremento: cierre S0 y aceptación restante M2A

- Leer M2_SEARCH_PLUS y EXTENSION_ARCHITECTURE. S1 ya tiene búsqueda literal por
  lotes con globs, snapshots abiertos, cancelación/generaciones y navegación.
  Preview de reemplazo no escribe; F9/Ctrl+Enter confirman, un archivo por turno,
  conflictos explícitos, abiertos sin guardar y cerrados mediante FileSession.
- Preservar límites de 2 MiB/archivo, snapshots de 8 MiB/petición y una pendiente,
  1000 filas/2 MiB por defecto y cola de cuatro lotes más evento terminal.
  Worker solo lee. No reemplazar desde renderer ni worker.
- S0 de documento todavía reconstruye consultas síncronamente, con case-insensitive
  ASCII. Implementar cotas/cancelación y Unicode case folding real antes de anunciar
  cierre de Search Plus; regex requiere motor real con presupuesto. Retirar la ruta
  sustituida, sin motores paralelos ni índices permanentes por defecto.
- No aumentar cuotas, activar regex o anunciar case folding completo por intuición.
  Datos y límites del incremento en S1_PROJECT_SEARCH. La validación Linux y una
  matriz manual amplia de corpus/foco/grafemas/terminales siguen abiertas.
- Guardado Windows conserva retries de 32/33/1175 con checks de huella; 1176/1177
  retienen temporal para recuperación. El proceso/handle histórico no está identificado.
  No sustituir por delete+rename ni limpiar recovery automáticamente.
- Prototipo GUI según PLAN con los mismos comandos/stores y comparación medida de
  backends. Sublime Text es referencia visual; ecode, referencia de operación.
  No duplicar buffer/undo. Progress/AVM real sigue M3.

## Medición y límites

S1 pasó 174 pruebas locales Windows, fmt, Clippy y build release (ver VALIDACION). Benchmarks opt-in:
`f3_pipeline`, `f3_jobs`, `project_search_bench`; smoke Windows `scripts/smoke-conpty.ps1 -ProjectSearch`, tras compilar
`f3_console_host` y `jc-ide-tui` en release. Método en S1_PROJECT_SEARCH y F3_PRESENTATION_TASKS.

ConPTY registra inyección-a-VT (p95 16,0598 ms en fixture mínimo), no pantalla
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
