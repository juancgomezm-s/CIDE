# Plan de desarrollo

Los hitos son secuenciales por evidencia, no por fechas.

## M0 — Base verificable

Núcleo byte-preserving, undo/redo, perfiles multilenguaje, puente Progress inicial, documentación y pruebas de base.

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

## M1 — Archivo y texto interpretado — ✅ CERRADO

**Alcance cerrado:** interpretación explícita de fuentes sin convertir el almacenamiento canónico, guardado seguro, posiciones textuales, cambios externos y límites del historial.

**Criterios cumplidos:**

- UTF-8 se reconoce por BOM o validez; un archivo ASCII se marca como compatible/ambiguo y no como prueba de una code page concreta.
- Un archivo legacy que no sea UTF-8 **no se adivina**. El proyecto/documento debe seleccionar explícitamente Windows-1252, ISO-8859-1 o IBM850; el catálogo es ampliable.
- Decodificar nunca modifica los bytes canónicos. Secuencias inválidas se muestran como incidencias con su rango de bytes.
- Insertar texto usa encoding estricto: un carácter no representable produce error y no altera el documento.
- CRLF, LF, CR, mezcla de finales y presencia/ausencia de newline final se inspeccionan sin normalización.
- Existe mapeo comprobado entre offset de byte y posición lógica línea/columna, incluyendo UTF-8 multibyte y CRLF.
- `FileSession` detecta contenido modificado o eliminado externamente antes de guardar.
- Guardado existente usa temporal en el mismo directorio + flush/fsync + reemplazo atómico: `rename` en Unix y `ReplaceFileW` en Windows.
- Archivos nuevos usan commit no-clobber; un nombre creado por otro proceso no se sobrescribe silenciosamente.
- Un fallo durante la escritura deja intacto el archivo original.
- Undo/redo queda limitado a 1024 pasos por defecto, configurable incluso a cero; la piece table expone compactación explícita del buffer añadido.
- Matriz Windows/Ubuntu validada con tests, Clippy sin warnings, release build y demo.

La página de códigos **real de cada proyecto de producción** no se codifica globalmente en JC-IDE: se registra en el perfil del proyecto. Esto evita convertir una incertidumbre de configuración en una heurística destructiva. Véase [M1_FILES_TEXT.md](M1_FILES_TEXT.md).


## M2A — TUI funcional — EN DESARROLLO

**Alcance de productividad ABL:** la edición y el snippet engine son universales; el contenido ABL y sus servicios especializados pertenecerán a una extensión de lenguaje interna y opcional. Tokenización/formateo se prepara en M3; firmas, USING y herencia en M4. Ver [arquitectura de extensiones](EXTENSION_ARCHITECTURE.md) y [comandos ABL](ABL_EDITOR_COMMAND_SCOPE.md).

**Search Plus es prioridad de producto:** primero documento activo, luego búsqueda en proyecto cancelable, más tarde referencias apoyadas en compilador XREF y catálogo de esquema. Ver [plan Search Plus](M2_SEARCH_PLUS.md).

Atajos acordados: Ctrl+D duplicar, Ctrl+Shift+D eliminar línea, Tab/Shift+Tab sangría, Ctrl+/ comentar/descomentar, Ctrl+Shift+/ bloque. La selección columna requiere rangos rectangulares reales y fallback por Palette cuando el terminal no entrega combinaciones. No confundir código incorporado con funcionalidad ya validada.

Existe `jc-tui` con ejecutable `jc-ide-tui`, entrada de terminal mediante crossterm y contrato de aplicación compartido. La presencia de código o pruebas parciales no equivale a aceptación funcional completa. Implementar y verificar cada flujo de extremo a extremo antes de cerrar este hito.

**Checkpoint actual:**
- edición real sobre `Workbench`, sin buffer/undo paralelo;
- Explorer de proyecto navegable y restringido a su raíz;
- resultados de búsqueda navegables desde el panel inferior;
- Duplicate/Delete Line/Move Line/Toggle Comment implementados como comandos reales;
- menús, palette, foco y pestañas ya consumen el contrato central;
- CI remoto permanece dormido hasta M5; este checkpoint no se declara validado por Actions.

**Nuevo incremento M2A — dos grupos de editor, navegación y Git read-only:** se incorporó un modelo inicial `PaneManager` con Primary/Secondary, referencia anclada, ubicación por bytes, historial atrás/adelante por grupo y toggle de scroll vinculado. Todavía falta conectarlo al renderer TUI, conservar cursor/scroll/selección por vista, navegar símbolos e includes y construir diffs. Las acciones de Git con escritura (stage/commit/push/pull/merge) quedan en M5. Ver [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).

**Pendiente para cerrar M2A:** búsqueda/reemplazo de proyecto, integración estructurada de Problems/Output con tareas Progress, revisión manual completa de teclado en terminal real y cualquier corrección de compilación/formato acumulada para hardening M5.

**Criterios de aceptación:**
- uso completo por teclado;
- abrir/guardar/cerrar, pestañas, búsqueda, sidebar, panel inferior y command palette;
- toda acción de negocio resuelve un `CommandId`;
- menús muestran el shortcut real;
- Progress Check/Compile/Run/Schema presentan diagnósticos sin bloquear edición;
- el frontend no crea un segundo buffer ni su propio undo.

## M2B — GUI Windows mínima — PENDIENTE

Elegir tecnología gráfica con mediciones y consumir el mismo `jc-app`.

**Criterios de aceptación:** abrir/editar/guardar, mouse, portapapeles, IME, accesibilidad, pestañas y paneles sin duplicar comandos ni lógica de aplicación. La GUI consume exactamente el mismo PaneManager (Split, Pin, navegación, DiffModel), añadiendo Ctrl+Click y drag en lugar de su propia lógica.

## M3 — Entorno Progress funcional

Estabilizar el contrato interno de capacidades para vincular proveedores de lenguaje sin introducir una runtime genérica de plugins; comenzar el paquete integrado opcional `progress.abl`, manteniendo el puente existente `jc-progress`. Validar OpenEdge 10.0B y 11.7, includes, PROPATH, aliases, code pages, compilación y ejecución en contexto real. Resolver includes `{...}` y RUN estáticos desde el perfil del proyecto sin ejecutar el programa para navegar; señalar destinos dinámicos como inciertos. La AVM solo se ejecutará a petición del usuario.

## M4 — Esquema y asistencia ABL

Como capacidades de la extensión `progress.abl`, desarrollar catálogo local, navegación, completado inteligente de tablas/campos/índices y análisis contextual incremental. Añadir Outline/Breadcrumb/Go to Definition/Open to Side mediante `NavigationTarget` universal y proveedores por lenguaje. Buscar/seleccionar/editar sigue siendo responsabilidad genérica de `jc-app` y Search Plus; XREF, imports USING y jerarquía OO son aportaciones del proveedor ABL.

## M5 — Servicios opcionales, hardening y CI

Git completo (stage/unstage por hunk, commit, fetch, pull, push, ramas, merge y resolución BASE/OURS/THEIRS/RESULT), proveedores semánticos/LSP, tareas e integración con OmniCore, todos bajo demanda. Construir los flujos de Git con confirmaciones y protección de archivos modificados; el visor read-only se adelanta a M2A. Hardening del contrato de proveedores internos de lenguaje y sus contribuciones al registro central; búsqueda de proyecto permanece priorizada en M2A.

**CI pertenece a M5.** Hasta iniciar M5 no se ejecutará GitHub Actions automáticamente en `push` ni `pull_request`, ni se usará CI remoto para perseguir fallos intermedios de M2–M4. El workflow queda conservado únicamente como `workflow_dispatch` manual y no debe invocarse antes de M5 salvo autorización explícita del propietario.

Durante M2–M4 la validación es local y dirigida: ejecutar solo las pruebas/builds necesarias cuando exista toolchain disponible. Si el entorno de desarrollo no permite validarlas, se registra la limitación y se continúa; no se consume CI para compensarla.

**Criterios de aceptación de CI en M5:**
- estabilizar primero la arquitectura y los flujos funcionales de M2–M4;
- reactivar entonces CI automático para `push`/`pull_request`;
- matriz Windows/Linux para formato, tests, Clippy y release build;
- smoke tests representativos de TUI, archivos y adaptadores que ya estén funcionalmente terminados;
- corregir en esa fase las incompatibilidades acumuladas entre plataformas, en lugar de pagar ese coste después de cada épica.

## Anexo post-M5 — Compatibilidad con Sublime Text

Después de cerrar M5 se abrirá un trabajo separado para compatibilidad con plugins, extensiones y paquetes de Sublime Text. **No forma parte de M5 y no debe condicionar M2–M5.**

La estrategia será incremental: primero formatos declarativos de bajo riesgo, después carga de paquetes y finalmente una capa de compatibilidad controlada para plugins Python. El objetivo no es clonar Sublime Text ni prometer compatibilidad binaria/API total, sino reutilizar una parte material de su ecosistema sin romper la arquitectura de JC-IDE.

Especificación: [POST_M5_SUBLIME_COMPAT.md](POST_M5_SUBLIME_COMPAT.md).

## Presupuestos exploratorios

- editor base: objetivo inicial <50 MiB de memoria privada;
- arranque: objetivo inicial <0,5 s hasta poder editar;
- Progress y otros procesos: medir por separado y como total;
- archivos grandes: medir latencia, piezas, historial y caches antes de optimizar.

Son objetivos, no resultados medidos.
