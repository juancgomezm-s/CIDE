# Continuación local — fundamentos de JC-IDE

Actualizado el 2026-11-01 después de implementar F1 sobre la auditoría ecode/eepp (`d7dcb69`). Este texto sustituye el handoff anterior que priorizaba terminar splits y después includes/RUN. La rama local es `m2a/dual-pane-tui`; consulta su log para el commit de F1. No asumir que está integrada en `main` o publicada.

## Instrucción para continuar

Continúa JC-IDE en Windows desde el estado real del clon. El objetivo es una base de edición eficiente y una GUI con dirección visual de Sublime Text; ecode es una referencia adicional de lógica, no un motor que deba copiarse entero. **F1 ya está implementado y validado**; lee [su checkpoint](F1_DOCUMENT_VIEWS_TRANSACTIONS.md). La siguiente implementación es **M2-F / F2**, acceso por rangos, índices y presupuestos de memoria.

1. Inspecciona `git status`, rama y log. Conserva cambios locales; no hagas reset/clean ni sustituyas la rama por `main`. Consulta el remoto solo si hace falta para integrar; no dispares Actions.
2. Lee `AGENTS.md`, `README.md`, `docs/AUDITORIA_ECODE_ARQUITECTURA.md`, `docs/ARQUITECTURA.md`, `docs/PLAN.md`, `docs/DECISIONES.md`, `docs/TUI_INTERACTION.md`, `docs/EXTENSION_ARCHITECTURE.md` y `docs/VALIDACION.md`.
3. Revisa `jc-core/document.rs`, `jc-text/index.rs`, `jc-app/panes.rs`, `jc-app/tabs.rs`, `jc-app/workbench/*` y `jc-tui/app/paths.rs`/`render/mod.rs` (todos bajo `crates/*/src/`). El ejemplo `crates/jc-app/examples/audit_baseline.rs` reproduce fallos y mide dos operaciones; no es una prueba de aprobación.
4. Usa `scripts/cargo-local.ps1` si el toolchain aislado sigue disponible; no modifica el PATH. Valida localmente y registra resultados. El sandbox puede bloquear hard links/ReplaceFileW en pruebas de archivo; no cambies el algoritmo para hacerlas pasar.

## Qué existe y qué falta

Existe core byte-preserving, codecs explícitos, sesión/guardado seguro, perfiles, comandos/keymap/foco y TUI con dos grupos, pin, back/forward, selección/viewport y undo compartido. El incremento funcional obtuvo 78 pruebas y smoke ConPTY en Windows; esa evidencia no cubre los defectos posteriores de la auditoría.

F1 eliminó la propiedad duplicada de cursor/selección/viewport; replace-all es una transacción. CRLF, dirty al reabrir y elección legacy están cubiertos por regresiones. Todavía se decodifican/indexan archivos completos en operaciones frecuentes. No hay GUI, motor de proyecto cancelable, resolver ABL, DiffModel ni AVM validada.

## F1 implementado: contrato que se debe conservar

- Separar `DocumentId`, `ViewId` y grupo. Un documento/sesión/undo; selecciones y viewport exclusivamente en la vista. Eliminar campos espejo de `EditorDocument` y ciclos guardar/restaurar usados para simular esa separación. La barra de pestañas pasa a ser presentación por grupo según el contrato nuevo.
- Introducir `EditBatch` con revisión esperada y validación completa previa. Un commit, un undo y un `ChangeSet` que permite reubicar todas las vistas; undo/redo emiten mapas completos. Retirar accesos de consumidores que muten contenido evitando la notificación.
- Centralizar límites de edición textual: no caret ni borrado parcial dentro de CRLF; conservar bytes y distinguir offsets de columnas/grafemas. No sustituir CRLF por LF para simplificar.
- Corregir apertura/elección de encoding y cierre/descarte/reapertura con transiciones explícitas. Dirty se deriva del documento. No restaurar texto descartado mientras se muestra una pestaña limpia.
- Migrar consumidores y quitar la ruta antigua en el mismo incremento. Mantener operativa la TUI; no conservar dos motores finales.

Aceptación cubierta por pruebas locales: replace-all de varias coincidencias se revierte en un undo; lote inválido/obsoleto deja estado intacto; bytes/BOM/EOL/encoding se preservan; caret CRLF válido; dos vistas comparten contenido/undo sin mezclar selección; cierre de una vista no destruye la otra; dirty y elección legacy correctos. Añadir regresiones observables para estos casos y ejecutar pruebas proporcionales al cambio.

## Después de F1

F2 incorpora acceso a segmentos e índices incrementales/cachés con presupuesto, y compara estructuras de almacenamiento bajo carga antes de elegir. F3 separa actualización de render, dibuja solo lo visible, emite diffs de terminal y recibe jobs versionados/cancelables. Medir p50/p95, trabajo recorrido y memoria desde ahora, sin CI remoto.

Tras estabilizar F1/F2, prototipar GUI con el mismo motor y la escena visual acordada. Search Plus de proyecto y navegación/Git read-only se construyen sobre F1–F3; la integración Progress real permanece en M3/M4. Git con escritura y CI en M5; plugins Sublime/Python post-M5.

## Límites y entrega

- No introducir pérdidas de bytes, normalización silenciosa ni encoding inferido destructivamente.
- No iniciar AVM, LSP, agentes o indexadores permanentes para abrir/editar.
- Mantener Ctrl+D duplicar, Ctrl+Shift+D borrar línea y reglas contextuales de foco.
- No disparar GitHub Actions ni publicar/mezclar cambios como efecto de pruebas.
- Entregar código y regresiones del incremento, evidencia local, límites, documentación coherente y commit local sin reescribir historial. No declarar cerrados M2 ni las metas de RAM/latencia sin evidencia.

Si la implementación requiere cambiar un contrato de la arquitectura, documenta el motivo y su coste; no añadas una sincronización compensatoria y la presentes como cierre del problema.
