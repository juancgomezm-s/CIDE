# Plan de desarrollo

Los hitos son secuenciales por evidencia, no por fechas.

## M0 — Base verificable

Núcleo byte-preserving, undo/redo, perfiles multilenguaje, puente Progress inicial, documentación y CI.

## M0.5 — Contrato de interacción

**Alcance:** formalizar la interacción antes de dibujar la TUI.

**Criterios de aceptación:**
- registro único de comandos y condiciones de disponibilidad;
- keymap contextual sin duplicados dentro del mismo scope;
- menús que derivan sus shortcuts del keymap;
- foco primario Editor/Sidebar/BottomPanel y overlays apilados;
- `Esc`, `F6`, pestañas, panel inferior y diálogos cubiertos por pruebas;
- shortcuts de Progress deshabilitados fuera de ABL/perfil;
- especificación normativa en `docs/TUI_INTERACTION.md`.

## M1 — Archivo y texto interpretado

Identificar las codificaciones reales, guardado seguro, posiciones textuales, detección de cambios externos y límites del historial.

## M2A — TUI funcional

Implementar `jc-tui` sobre `jc-app`.

**Criterios de aceptación:**
- uso completo por teclado;
- abrir/guardar/cerrar, pestañas, búsqueda, sidebar, panel inferior y command palette;
- toda acción de negocio resuelve un `CommandId`;
- menús muestran el shortcut real;
- Progress Check/Compile/Run/Schema presentan diagnósticos sin bloquear edición;
- el frontend no crea un segundo buffer ni su propio undo.

## M2B — GUI Windows mínima

Elegir tecnología gráfica con mediciones y consumir el mismo `jc-app`.

**Criterios de aceptación:** abrir/editar/guardar, mouse, portapapeles, IME, accesibilidad, pestañas y paneles sin duplicar comandos ni lógica de aplicación.

## M3 — Entorno Progress funcional

Validar OpenEdge 10.0B y 11.7, includes, PROPATH, aliases, code pages, compilación y ejecución en contexto real.

## M4 — Esquema y asistencia ABL

Catálogo local, navegación, completado de tablas/campos/índices y análisis contextual incremental.

## M5 — Servicios opcionales

Git, búsquedas de proyecto, proveedores semánticos/LSP, tareas e integración con OmniCore, todos bajo demanda.

## Presupuestos exploratorios

- editor base: objetivo inicial <50 MiB de memoria privada;
- arranque: objetivo inicial <0,5 s hasta poder editar;
- Progress y otros procesos: medir por separado y como total;
- archivos grandes: medir latencia, piezas, historial y caches antes de optimizar.

Son objetivos, no resultados medidos.
