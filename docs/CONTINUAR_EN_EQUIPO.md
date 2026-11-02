# Continuación local — después de F3

F1/F2/F3 están implementados con evidencia local en `m2a/dual-pane-tui`. F1:
`ddc9d41`; F2: `2423134` + normalización `e72856f`; consultar el log para F3.
No sustituir el árbol por main ni repetir los checkpoints anteriores.

## Inicio

1. Revisar git status/rama/log y conservar modificaciones del usuario.
2. Leer AGENTS, README, PLAN, ARQUITECTURA, DECISIONES, los checkpoints F1/F2/F3
   y VALIDACION, bajo `docs/`. Mediciones crudas en `docs/benchmarks/`.
3. Revisar `jc-app/src/jobs.rs`, `workbench/text.rs`, `panes.rs`,
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

## Siguiente incremento funcional: Search Plus de proyecto

- Leer M2_SEARCH_PLUS y EXTENSION_ARCHITECTURE antes de integrar proveedores.
- Añadir identidad/generación de contexto de proyecto y escaneo por lotes acotados,
  cancelables; preferir bytes sucios abiertos sobre disco. No clonar cada archivo
  completo por tecla ni iniciar un indexador permanente.
- Resultados tipados con coordenadas/revisión/encoding explícitos; UI entrega sin
  otra tecla. Preview de reemplazo y conflictos antes de editar; atomicidad por
  documento/archivo, no prometer transacción global del filesystem.
- Search de documento todavía reconstruye consultas síncronamente; migrar la ruta
  sustituida cuando corresponda, sin conservar dos motores productivos paralelos.
- El contrato F3 fue probado con proveedores de fixture; Progress real pertenece a
  M3. No habilitar Check/Compile/Run/Schema sin proveedor y evidencia real.
- Prototipo GUI según PLAN sobre los mismos comandos/stores, con comparación
  medida de backends. Sublime Text sigue como dirección visual; ecode es referencia
  de operación. No elegir backend por intuición ni duplicar buffer/undo.

## Medición y límites

F3 pasó 157 pruebas locales, fmt, Clippy y build release. Benchmarks opt-in:
`f3_pipeline`, `f3_jobs`; smoke Windows `scripts/smoke-conpty.ps1`, tras compilar
`f3_console_host` y `jc-ide-tui` en release. Método en F3_PRESENTATION_TASKS.

ConPTY registra inyección-a-VT (p95 16,055 ms en fixture mínimo), no pantalla
física. Un muestreo corto de memoria/CPU de ese proceso no certifica metas globales.
Mantener abiertas latencia física, arranque, RAM/picos con varios documentos,
servicios y doble grupo nativo. Investigar si reaparece el error intermitente
Windows 1175 de ReplaceFileW registrado en VALIDACION: la repetición dirigida y
la siguiente corrida completa pasaron, causa aún no identificada.
F2 ya midió rangos/modelo hasta 16 MiB; no confundir
esas llamadas con latencia UI. Revisar líneas extremas, metadatos O(L), muchas
piezas y ancho Unicode/grafemas/IME/DPI. M2A continúa abierto.

Conservar bytes, BOM/EOL/CRLF, encoding explícito, guardado seguro, revisiones y
vistas independientes. No tocar Qwen, GPU, drivers, startup ni servicios ajenos.
No hacer push ni operaciones remotas sin autorización. Git con escritura y CI en
M5; plugins externos post-M5. Entregar evidencia y commit local sin reescribir
historial.
