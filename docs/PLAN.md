# Plan de desarrollo

Los hitos son secuenciales por evidencia, no por fechas.

**Revisión 2026-11-01:** la [auditoría de JC-IDE y ecode](AUDITORIA_ECODE_ARQUITECTURA.md) introduce una puerta de arquitectura antes de ampliar M2. Sublime Text continúa como referencia visual; ecode/eepp aporta patrones de operación, no un motor que se vaya a portar completo. La funcionalidad ya probada se conserva, pero no acredita rendimiento ni cierre de los defectos descubiertos.

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

## M1 — Archivo y texto interpretado — cierre histórico de alcance

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

El cierre histórico no cubre todas las rutas integradas de M2. La auditoría reproduce navegación dentro de CRLF y retención de bytes no limitada por el número de undos. Se corrigen en F1/F2, conservando las pruebas de archivo/encoding y sin describir M1 como certificación de escalabilidad.

## M2-F — Fundamentos antes de ampliar funciones — PRIORIDAD ACTUAL

**F0, completado en esta revisión:** evidencia estática, reproducción de fallos, baseline local y arquitectura de destino documentada. El ejemplo `audit_baseline` no certifica corrección ni modifica el editor. **F1 implementado y validado localmente:** propiedad única de estado de vista, identidades separadas, lotes atómicos y correcciones de apertura/cierre/CRLF. **F2 implementado y validado:** rangos/segmentos, proyección por revisión/encoding, checkpoints y EOL incrementales, render visible, cuotas y recolección conservando historial. [Mediciones, elección de bloques y límites](F2_RANGES_INDEX_MEMORY.md). F3 implementado con evidencia local de presentación de solo lectura, terminal incremental y trabajos internos acotados/versionados; [método y límites](F3_PRESENTATION_TASKS.md). Objetivos globales de RAM/arranque/tecla-a-pantalla abiertos; ConPTY mide inyección-a-VT, no pantalla física. El [checkpoint F1](F1_DOCUMENT_VIEWS_TRANSACTIONS.md) precisa APIs, política de posiciones y diferencias entre el catálogo TUI y las vistas por grupo.

| Incremento | Entrega concreta | Condición de salida |
|---|---|---|
| F1: propiedad y cambios — implementado | `DocumentId`/`ViewId`/grupos, un estado por vista, `EditBatch` atómico, `ChangeSet` completo, transiciones explícitas de apertura/cierre | Retirar cursor/scroll espejo y mutación que evita notificaciones. Un undo revierte replace-all; fallo de lote no altera nada; CRLF no admite caret interno; encoding legacy permanece seleccionable; dirty/reabrir coherentes; dos vistas comparten contenido/undo y conservan selecciones. |
| F2: acceso y memoria — implementado con mediciones locales | Segmentos/snapshots, índice textual por revisión, cachés con cuota, presupuesto de undo por bytes | Navegación y lectura de viewport estable sin copia/decode global. Comparación de estructuras bajo la misma carga; demostrar escalado y liberación de bytes no referenciados sin perder historial prometido. |
| F3: render y tareas — implementado, aceptación global de rendimiento abierta | Modelo visible de solo lectura, diff de celdas, loop con eventos de jobs, resultados versionados/cancelables | Dibujar no muta Workbench; UI recibe resultados sin otra tecla; tareas obsoletas no pisan estado; pantalla/listas virtualizadas; medición de latencia completa y cancelación. |

Cada incremento sustituye y retira su ruta antigua; no mantener dos modelos finales ni añadir sincronizaciones compensatorias. Conservar core byte-preserving, guardado, perfiles, contratos de interacción y regresiones útiles. Los nuevos tests deben cubrir comportamiento y fallos reales, no reflejar internals.

Dependencias: F1 → F2 → F3 → ampliación funcional M2A. El prototipo de backend GUI M2B puede empezar al estabilizar F1/F2 y madurar con F3; no depende de haber implementado toda la semántica ABL. No usar servicios ni índices permanentes para compensar lentitud del camino básico de edición.


## M2A — TUI funcional — EN DESARROLLO

**Alcance de productividad ABL:** la edición y el snippet engine son universales; el contenido ABL y sus servicios especializados pertenecerán a una extensión de lenguaje interna y opcional. Tokenización/formateo se prepara en M3; firmas, USING y herencia en M4. Ver [arquitectura de extensiones](EXTENSION_ARCHITECTURE.md) y [comandos ABL](ABL_EDITOR_COMMAND_SCOPE.md).

**Avance S1 del 2026-11-02:** búsqueda literal de proyecto, filtros, snapshots abiertos, lotes/generaciones/cancelación, navegación y preview/reemplazo por archivo implementados con evidencia local. Abiertos usan un undo y no se guardan automáticamente; cerrados usan FileSession, con conflictos explícitos. [APIs, cuotas, mediciones y límites](S1_PROJECT_SEARCH.md). No cierra S0 (case folding Unicode completo, búsqueda de documento acotada/regex real), matriz funcional ni aceptación global de rendimiento. El tratamiento de guardado Windows 32/33/1175 y recuperación 1176/1177 está documentado en [SAVE_WINDOWS_CONTENTION.md](SAVE_WINDOWS_CONTENTION.md).

**Avance S0 del 2026-11-02:** búsqueda literal cooperativa por rangos, default full case folding Unicode 17, palabra Unicode, selección/historial/marcadores y Go to escalar implementados. Un matcher compartido sustituye el scan síncrono ASCII y el KMP separado de proyecto. [Evidencia y coste](S0_DOCUMENT_SEARCH.md): suite de 186 pruebas más regresiones dirigidas de vistas, límites efectivos y recorridos Windows. La subida inicial de ~35 % del p95 de proyecto se abordó con decoder prestado, scratch reutilizado y rechazo ASCII en el mismo matcher: [bloque medido](S0_DECODER_OPTIMIZATION.md). En el corpus acordado p95 final 18,649 ms frente a 40,930 ms contemporáneos y 27,457 ms S1; cancelación sin mejora consistente. Suite final completa de 191 pruebas y recorridos nativos. Regex con presupuesto real, matriz manual y aceptación global siguen abiertas.

**Avance de evidencia M2 del 2026-11-02:** [corpus ampliado y procesos](M2_ACCEPTANCE_EVIDENCE.md), 194 pruebas completas, Unicode/legacy, 4097 piezas y ocho documentos/dos vistas. Se corrigió la materialización completa del layout frío: pico real de línea ASCII de 16 MiB 434,9 → 18,0 MiB. Arranque con caché del SO no vaciada p95 0,064/0,114/0,204 s (siete procesos por forma). Modelo de ocho documentos con líneas muy cortas aún ~69,5 MiB privados; metadatos densos y aceptación física/global permanecen abiertos.

**Search Plus es prioridad de producto:** primero documento activo, luego búsqueda en proyecto cancelable, más tarde referencias apoyadas en compilador XREF y catálogo de esquema. Ver [plan Search Plus](M2_SEARCH_PLUS.md).

Esta prioridad se ejecuta sobre M2-F: antes de sumar búsqueda de proyecto deben existir transacciones, resultados por revisión y presupuesto de trabajo/memoria. Navegación de proveedores, Git y asistentes no se anteponen a esa base.

Atajos acordados: Ctrl+D duplicar, Ctrl+Shift+D eliminar línea, Tab/Shift+Tab sangría, Ctrl+/ comentar/descomentar, Ctrl+Shift+/ bloque. La selección columna requiere rangos rectangulares reales y fallback por Palette cuando el terminal no entrega combinaciones. No confundir código incorporado con funcionalidad ya validada.

Existe `jc-tui` con ejecutable `jc-ide-tui`, entrada de terminal mediante crossterm y contrato de aplicación compartido. La presencia de código o pruebas parciales no equivale a aceptación funcional completa. Implementar y verificar cada flujo de extremo a extremo antes de cerrar este hito.

**Checkpoint actual:**
- edición real sobre `Workbench`, sin buffer/undo paralelo;
- Explorer de proyecto navegable y restringido a su raíz;
- resultados de búsqueda navegables desde el panel inferior;
- Duplicate/Delete Line/Move Line/Toggle Comment implementados como comandos reales;
- menús, palette, foco y pestañas ya consumen el contrato central;
- CI remoto permanece dormido hasta M5; este checkpoint no se declara validado por Actions.

**Incremento M2A — dos grupos funcionales (2026-11-01):** `PaneManager` ya está conectado al renderer TUI, comandos, menús y palette. Cursor, selección y viewport se conservan por grupo; las ediciones/undo/redo ajustan posiciones de la otra vista e historial sin duplicar documentos. Incluye pin, apertura lateral, back/forward, scroll vertical vinculado y fallback al grupo activo con poco ancho. Hay pruebas locales y smoke test en terminal Windows; véase [VALIDACION.md](VALIDACION.md). Siguen pendientes símbolos/includes, Git read-only/diffs y revisión manual completa. Git con escritura permanece en M5. Ver [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).

**Pendiente para cerrar M2A:** aceptación global de rendimiento de M2-F, picos/sufijos restantes y ampliar frío/teclado sobre el corpus ya medido, regex real si se incorpora y matriz Search Plus, integración de proveedores reales sobre el contrato de tareas F3 y revisión manual completa de teclado. El incremento literal de proyecto con preview y conflictos ya existe; no se confunde con cierre completo de Search Plus. El contrato de tareas/Problems/Output ya tiene proveedor de prueba y regresiones locales; no equivale a Progress conectado. La ejecución real de Progress y su validación pertenecen a M3; no se aplazan defectos conocidos de datos/edición hasta M5.

**Criterios de aceptación:**
- uso completo por teclado;
- abrir/guardar/cerrar, pestañas, búsqueda, sidebar, panel inferior y command palette;
- toda acción de negocio resuelve un `CommandId`;
- menús muestran el shortcut real;
- el contrato de tareas entrega diagnósticos sin bloquear edición y descarta resultados obsoletos; Progress Check/Compile/Run/Schema permanecen deshabilitados si no hay proveedor validado;
- el frontend no crea un segundo buffer ni su propio undo.

**Bloque posterior de RAM/teclado:** [metadatos compactos](M2_DENSE_LINE_METADATA.md), recolección que reutiliza arenas enteras y seis regresiones de teclado; 209 pruebas completas y recorridos nativos. Modelo denso ~49,3 MiB tras frío; pico nativo 71,9 → 51,7 MiB, por lo que la puerta global permanece abierta. Frío/sufijos tienen costes registrados. El dispatcher TUI ya impide ejecutar menús deshabilitados; [matriz y pendientes físicos](M2_KEYBOARD_MATRIX_AUDIT.md).

**Preparación paralela autorizada del 2026-11-02:** [DiffModel puro](DIFF_MODEL_READ_ONLY.md) de fuentes inmutables, hunks y correspondencia de líneas exactas, con cuotas/cancelación; seis regresiones y suite integrada de 200 pruebas. No hay comandos/renderer/proveedor Git conectados ni rendimiento diff aceptado. Los picos/sufijos densos restantes y las puertas globales de M2-F siguen prioritarios.

**Proveedor read-only posterior:** [Git HEAD/index](GIT_READ_ONLY_PROVIDER.md), Windows con snapshots de bytes exactos/identidad, límites y contención/cancelación. Nueve regresiones efectivas y suite completa de 218 pruebas; composición con DiffModel verificada. Aún no hay entrega por revisión/generación, UI/status ni comandos Git funcionales. Su RAM/latencia auxiliar siguen por medir antes de aceptación global. Escrituras Git permanecen M5.

**Integración posterior del 2026-11-02:** [entrega Git versionada](GIT_DIFF_DELIVERY.md) y [visor TUI](GIT_DIFF_PRESENTATION.md) conectados para el archivo activo. HEAD/index se comparan con el snapshot del buffer sucio mediante el JobSystem existente; un trabajo en vuelo, pendiente de metadatos, cuotas y descarte por revisión/encoding/vista/ruta/generación. Comandos centrales de configuración, refresh y cancelación, lista de hunks, navegación y diff unificado visible por rangos. 237 pruebas locales aprobadas al cierre, fmt/Clippy/release y recorrido ConPTY HEAD/index sin otra tecla, stale/undo y conservación de disco/index. La ronda inicial detectó una expectativa antigua de menú; se corrigió y repitió solo TUI. No acredita status de repositorio, watcher, otras plataformas ni aceptación global RAM/pantalla; auxiliares Git aún por medir. Las menciones anteriores sin UI/jobs describen checkpoints históricos.

**Bloque siguiente medido:** [sufijos y crecimiento acotado](M2_SUFFIX_COST_REVIEW.md), 4914 observaciones antes y otras 4914 después, 240 pruebas completas y fmt/Clippy/release. Una primera newline ya no duplica el índice: capacidad posterior de proyección en el corpus de 8 MiB, 8388792→4227256 B. Se conserva el coste O(L) y el caso frío único 1760,8→2145,2 µs; no se declara mejora general de latencia/RAM. [Auxiliares Git](GIT_AUXILIARY_MEASUREMENT.md): nueve sesiones, sumas observadas por ciclo hasta 11,156 MiB privados en el corpus cercano a 2 MiB, excluyendo bootstrap/conhost inicial/harness; cobertura temporal limitada. Siguientes frentes activos: distribución de primeras ediciones y recorrido nativo denso con edición/undo en ocho documentos. No se cierra M2A ni la puerta global de 50 MiB/pantalla física.

**Cierre de esos dos frentes:** [tres pares de distribución fría](M2_COLD_GROWTH_DISTRIBUTION.md), 5580 filas, mismo ejemplo sobre `6cf6f13` y `d6de969`; y [14 procesos nativos retenidos](M2_DENSE_EDIT_EVIDENCE.md), mismo recorrido de edición/undo en ocho documentos. Pico histórico de compromiso 68,160→52,461 MiB; máximo privado observado tras editar 65,922→50,434 MiB. La puerta global sigue abierta. El p95 frío no cambia consistentemente de signo entre pares; no se declara aceleración ni regresión general. Sin cambio productivo ni repetición de suites sin necesidad. Ese bloque experimental se cerró en `191bd82`; su selección e integración posterior están documentadas a continuación.

**Bloque por bloques integrado y medido:** [diseño/evidencia](M2_BLOCK_LINE_INDEX.md),
una única representación productiva, retirado el comparador dependiente del plano
(reproducible en `191bd82`). Suite completa 245 aprobadas y revisión de capacidad
posterior con 15+3 pruebas afectadas: 246 distintas al cierre; fmt/Clippy/release.
Dos pares de Workbench con igual ejemplo, 48 procesos nativos / 42 retenidos y
1984 operaciones de pipeline. Pico histórico denso editado 52,461→43,727 MiB,
repetición 52,313→43,820 MiB. El movimiento caliente sube ligeramente; sparse/cold
tienen tradeoffs. [Undo por etapas](M2_UNDO_PIPELINE_PROBE.md): mejora interna en
ambos órdenes; p95 VT repetido 16,9702 ms plano / 16,9147 ms bloques, sin persistir
la diferencia inicial. Global RAM/servicios/pantalla física y M2A siguen abiertos.
Conservar evidencia, sin repetir suites por documentación ni cambios menores.

## M2B — GUI Windows mínima — PENDIENTE

Ya existe un [estudio estático de escena y contrato](M2B_SCENE_STUDY.md) preparado en paralelo: ABL/include PIN/Explorer/Search/palette, datos locales y matriz neutral. Sus comprobaciones estáticas pasan; la captura headless falló sin imagen, por lo que revisión renderizada sigue pendiente. No es editor gráfico ni decisión de backend.

Elegir tecnología gráfica con mediciones y consumir el mismo `jc-app`. Iniciar una escena de comparación al estabilizar F1/F2: fuente ABL + include en referencia anclada + Explorer + búsqueda + palette. Mantener la dirección visual de Sublime Text: editor dominante, marco compacto, tabs discretas, sidebar colapsable, panel inferior y barra de estado. Estilo visual no implica copiar shortcuts ni compatibilidad de plugins.

**Criterios de aceptación:** abrir/editar/guardar, mouse, portapapeles, IME, accesibilidad, pestañas y paneles sin duplicar comandos ni lógica de aplicación. La GUI consume los mismos stores/grupos de F1 (Split, Pin, navegación y posteriormente DiffModel), añadiendo Ctrl+Click y drag como entradas. Capturas y pruebas de foco/DPI 100/150/200 %, fuentes de ancho variable por grafema, dos vistas del mismo documento y comparación de latencia/RAM/arranque. Rechazar un backend que obligue a normalizar bytes o mantener otro buffer/undo editable. Minimap después del render visible eficiente, sobre resumen cacheado.

## M3 — Entorno Progress funcional

Estabilizar el contrato interno de capacidades para vincular proveedores de lenguaje sin introducir una runtime genérica de plugins; comenzar el paquete integrado opcional `progress.abl`, manteniendo el puente existente `jc-progress`. Validar OpenEdge 10.0B y 11.7, includes, PROPATH, aliases, code pages, compilación y ejecución en contexto real. Resolver includes `{...}` y RUN estáticos desde el perfil del proyecto sin ejecutar el programa para navegar; señalar destinos dinámicos como inciertos. La AVM solo se ejecutará a petición del usuario. Conectar Check/Compile/Run/Schema a Problems/Output a través de los jobs de M2-F; comprobar cancelación y que resultados de otra revisión no se apliquen silenciosamente.

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
- completar en esa fase la matriz transversal de compatibilidad; los defectos conocidos de integridad y respuesta se corrigen en el incremento que los introduce o descubre.

## Anexo post-M5 — Compatibilidad con Sublime Text

Después de cerrar M5 se abrirá un trabajo separado para compatibilidad con plugins, extensiones y paquetes de Sublime Text. **No forma parte de M5 y no debe condicionar M2–M5.**

La estrategia será incremental: primero formatos declarativos de bajo riesgo, después carga de paquetes y finalmente una capa de compatibilidad controlada para plugins Python. El objetivo no es clonar Sublime Text ni prometer compatibilidad binaria/API total, sino reutilizar una parte material de su ecosistema sin romper la arquitectura de JC-IDE.

Especificación: [POST_M5_SUBLIME_COMPAT.md](POST_M5_SUBLIME_COMPAT.md).

## Presupuestos y medición desde M2

- editor base: objetivo inicial <50 MiB de memoria privada;
- arranque: objetivo inicial <0,5 s hasta poder editar;
- Progress y otros procesos: medir por separado y como total;
- archivos grandes: medir latencia, piezas, historial y caches antes de optimizar.

Son objetivos exploratorios, no resultados alcanzados. La medición **local empieza en M2-F**; CI remoto continúa en M5. El perfil actual `opt-level="s"` prioriza tamaño: compararlo con 2/3 usando las mismas cargas antes de fijar la política de release. No cambiarlo por intuición.

Metas iniciales para un equipo de referencia identificado en cada corrida: p95 de movimiento/consulta textual <2 ms con índices calientes en archivos de hasta 4 MiB; p95 tecla-a-pantalla <16 ms con viewport fijo, una/dos vistas y servicios inactivos; cancelación de búsqueda perceptible <100 ms en la carga local. Son presupuestos para orientar F2/F3, no garantías universales. Ajustarlos solo con evidencia registrada; informar degradación en archivos/líneas extremas.

Matriz local: 4 KiB, 64 KiB, 1/4/16 MiB; líneas cortas y una línea larga; UTF-8 multibyte/combining/ancho doble, legacy, BOM y EOL mixtos; pocas/muchas piezas; búsquedas con cero/muchas coincidencias; edit/undo/redo y cerrar/reabrir. Registrar hardware, OS/toolchain/perfil, corpus, calentamiento, muestras y p50/p95; separar apertura fría, edición caliente y procesos auxiliares.

Medir también bytes decodificados, asignaciones por operación, memoria privada y retención tras liberar historial/cerrar documentos. Un viewport fijo no debe provocar trabajo proporcional a todo el archivo en cada tecla. Cuotas de workers, cola, resultados y cachés serán configuración explícita con valores derivados de esas mediciones. La primera evidencia y sus límites están en [AUDITORIA_ECODE_ARQUITECTURA.md](AUDITORIA_ECODE_ARQUITECTURA.md).


## Prioridad vigente: CPU y concurrencia acotada — 2026-11-02

Juan pide aprovechar núcleos disponibles. El bloque Git proveedor/controlador
está cerrado con 261 pruebas completas, fmt/Clippy/release; su presentación TUI
se pospone ante esta prioridad. Leer GIT_STATUS_READ_ONLY y GIT_STATUS_DELIVERY.

Primero inventariar hardware real y perfilar búsqueda por archivos, diff,
decodificación y tareas de fondo. Elegir el cuello medido; reutilizar planificador
cuando encaje, concurrencia configurable/acotada, cancelación, cuotas/backpressure,
resultados deterministas/versionados, sin pools anidados ni copias grandes.
Preservar bytes/encoding/undo y no paralelizar escrituras de un documento.

Antes/después con corpus/protocolo idénticos: throughput, latencia/p95 del tramo
medible, CPU por proceso/tiempo y memoria/picos. 100% CPU no es aceptación.
Reservar margen para Qwen; sin afinidades/prioridades/servicios/GPU ajenos ni
archivos privados. Si no puede verificarse su actividad sin interferir, registrar
carga concurrente desconocida y no atribuir toda variación al cambio. Máximo
tres agentes, un integrador, compilaciones y mediciones pesadas serializadas.
Construir bloques completos antes de validar. Sin instalaciones, push ni CI.
