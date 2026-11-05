# Plan de desarrollo

## Estado de cierre M2 — 2026-11-04

Requisito autorizado e integrado: [motor Git Rust embebido](GIT_EMBEDDED_ENGINE.md),
sin instalación de Git. `jc-git` enlaza gix 0.90.0 en GUI/TUI, reemplazando
el runner externo para estado y blobs HEAD/índice. Rust 1.88.0 está instalado
solo en el workspace; Cargo.lock y licencias de las transitivas están fijados.
Ejecutables nuevos: `target/embedded-git-next/release`. G0/G1/G2 de lectura
se cierran con la evidencia de VALIDACION.md y las limitaciones allí registradas.
Operaciones de escritura, red, merge/rebase y credenciales siguen en M5;
esta integración no anuncia paridad completa con Git.

[Integración Git automática](M2_GIT_INTEGRADO_OMNI.md), adaptada de los flujos
de OmniCore/OmniCoder: no se pide elegir ejecutable en GUI. Validación nativa
integral de 135 checks y binarios GUI/TUI en `target/git-integrated-next/release`.

Última corrección GUI: [estado Git accesible](M2_GIT_GUI_STATUS_UX.md), con
ejecutable elegido recordado, contador corregido y actualización por eventos.
Leyenda F6 retirada solo de GUI; atajo conservado. Binario GUI vigente en
`target/git-ux-next/release`, con pruebas y límites en la matriz de cierre.

Incremento vigente: [marco/pestañas/paneles](M2_SHELL_PANELS_MENUS.md) y
[menús depurados](M2_MENU_AUDIT.md). Se corrige la captura que impedía
redimensionar por el borde, se separa Código al lado derecho y se añaden
controles inferiores independientes para ambos laterales. Pestañas con
flechas a la izquierda, Nuevo/lista a la derecha y delimitadores. Estado de
validación detallado en la matriz de cierre; ejecutables en
`target/m2-panels-next/release`.

La [matriz de cierre](M2_CIERRE.md) reúne la evidencia y las puertas restantes.
Por indicación de Juan, Narrador y accesibilidad avanzada quedan pendientes.
M2 concentra el trabajo actual en interfaz, desplazamiento y edición.

El bloque de avisos y nombres de borradores sustituye la confirmación
clara de Windows por un diálogo propio con la paleta de CIDE y cancelación
inicial. Los nombres «Sin título» usan el primer número libre del catálogo
vivo, para reiniciar en una sesión vacía y conservar borradores recuperados.
La revisión posterior corrigió el tamaño y centrado: aviso de 420 × 176
píxeles de diseño, cuerpo más claro y borde visible. Pasaron la inspección
de escritorio, cancelación con Intro, reinicio de numeración y salida normal.
Ejecutable vigente: `target/m2-dialog-compact/release/cide-gui.exe`.

El pulido mantiene visible la pestaña activa al reducir ancho, escala las
barras con DPI y evita reconstruir el dibujo cuando la rueda o el arrastre
conservan la misma vista. El recorrido integral nativo pasó. Una medición
visible de 360 pasos de rueda sobre 4,4 MB dio 3,35 ms de mediana y
4,79 ms de p95 hasta confirmación de pintura.

AvPág/RePág ya usan las filas visibles del grupo activo, con selección
y panel inferior, y pasaron su regresión dirigida. Quedan pendientes la
latencia física, IME real y cambio entre monitores de distinta escala.
La validación de composición existente es inyectada. Los ejecutables de
este bloque están en `target/m2-polish-next/release`.

## Estado vigente — recuperación periódica y limpieza segura, 2026-11-03

Recuperación automática en Rust integrada en GUI y TUI: captura cooperativa de
64KiB por turno, cola de un tramo, escritura/verificación en worker y detección de
versiones obsoletas. Programa tras2s sin actividad y fuerza un intento a30s;
un documento pendiente que cambia cancela la captura incompleta. Fallos esperan10s.
Tras la captura inicial, una sesión sin cambios no reescribe sus copias.

La limpieza conserva HEAD/BACKUP verificados, borra únicamente generaciones
obsoletas del slot propio por handles Win32 y rechaza enlaces/archivos desconocidos.
Las escrituras incompletas propias tienen guarda RAII. La limpieza aplazada vuelve
a intentarse sin reconstruir documentos ni avanzar HEAD. El cierre normal espera
el worker y confirma después la última edición. [Contrato](PERIODIC_RECOVERY_RETENTION.md).

20 nuevas pruebas aprobadas por suites/cierres dirigidos, más regresiones de
recuperación y terminación abrupta. Tres procesos reales de GUI y tres de TUI
pasaron7 comprobaciones por interfaz con terminación forzada, fuente original
intacta, dos generaciones protegidas y ausencia de reescrituras en reposo.
Los enlaces duros/junction y Guardar como de fixtures necesitaron ventana local
sin la restricción del sandbox. No se cambiaron garantías para hacerlos pasar.
Fmt, Clippy todos los targets sin advertencias y release offline aprobados;
no corrida runtime global única. Evidencia: `docs/evidence/periodic-recovery-2026-11-03-*`.

Ejecutables actuales: `target/recovery-next/release/cide-gui.exe` y `cide-tui.exe`.
Se conservan los ejecutables anteriores. Depósitos personales intactos en pruebas.
No se modifican archivos originales salvo Guardar explícito. Siguen abiertos la
integración GB, aceptación física/global, IME/DPI/a11y/clipboard real y M3–M5.
Sin dependencias nuevas, CI, push ni publicación.

El siguiente avance es histórico y queda subordinado a este cierre.

## Estado previo — marco GUI, ratón TUI y recuperación Rust, 2026-11-03

GUI ahora tiene marco de título integrado, barra de menús con el mismo fondo
`#2E3440`, pestañas redondeadas por grupo, selector de documentos abiertos y árbol
en píxeles con fuente de interfaz separada del código. Reutiliza una superficie
gráfica y el frame preparado en exposiciones. [Construcción](M2B_GUI_TABS_BROWSER.md).
TUI incorpora ratón y explorador modal de archivos/carpetas, con ayuda en español:
[uso y alcance](M2_TUI_MOUSE.md).

Ambas interfaces recuperan documentos sin guardar, bytes originales, undo/redo,
pestañas y vistas al salir y volver a abrir. El almacén Rust usa generaciones
inmutables, sincronización, verificación por relectura y HEAD/BACKUP atómicos;
si falla la confirmación, el editor permanece abierto. [Contrato y límites](SESSION_RECOVERY.md).
No hay todavía checkpoint periódico ni recolección de generaciones antiguas.

Check de todos los targets, fmt, Clippy sin advertencias y release offline aprobados.
Pruebas dirigidas de recuperación de core/archivos/aplicación/GUI/TUI, terminación
abrupta de procesos y reapertura ConPTY aprobadas; no corrida runtime global única.
GUI:15 checks de shell y13 de interacción, cero errores de pintura; TUI: navegador5,
ratón7 y recuperación4 comprobaciones en tres procesos; GUI también restauró cambios,
caret e historial en tres procesos reales ocultos. Fuentes fixture intactas.
Evidencia: `docs/evidence/gui-shell-browser-2026-11-03-*`.

Ejecutables actuales: `target/gui-next/release/cide-gui.exe` y `cide-tui.exe`.
Los anteriores `target/release` y `target/ui-es/release` se conservan.
La fuente paginada aislada leyó un fixture disperso de5GiB; el editor completo aún
no admite GB. Se eligió instantánea privada y falta integrarla con Document y los
índices progresivos: [auditoría](CORE_GIGABYTES_ADMISSION.md).
Aceptación física/global, IME/DPI/a11y/clipboard real y proveedores M3–M5 abiertos.
Sin nuevas dependencias externas, CI, push ni publicación.

Los estados siguientes son históricos y no sustituyen este avance.

## Estado vigente — cierre funcional TUI, 2026-11-03

Los bloques admitidos de TUI están implementados y validados localmente: regex,
comparación de abiertos, símbolos/includes/RUN léxicos opcionales, completado
textual, snippets, foco/overlays, clipboard Windows explícito y layout Unicode.
[Matriz vigente](M2_TUI_FINISH.md), [contrato Unicode](M2_UNICODE_LAYOUT.md).
530 nombres de pruebas aprobadas por suites/cierres dirigidos,18 casos nuevos;
dos helpers Git se ejecutan por sus padres, no una corrida global única.
Fmt/Clippy/release offline, cuatro flujos ConPTY y17 checks HWND propios aprobados.

Se preservan bytes/encoding/BOM/EOL/undo, F1/F2/F3 y un único modelo editable.
Unicode: grafemas/narrow/TAB4, caché512+256+128+128KiB/documento, filas64KiB,
presentación fuente compartida2MiB, pantalla131072 celdas. Clipboard TUI tiene
hilo Win32 lazy con buzón único y propietario que responde inactivo; pruebas sin
abrir clipboard personal. Las dos bibliotecas autorizadas están fijadas y el
mínimo declarado es Rust1.85; toolchain existente1.85.1, sin instalar programas.

La siguiente puerta es aceptación física/global con terminal/fuente del usuario,
clipboard real y mediciones RAM/arranque/pantalla; no se cierra con modelo/ConPTY.
No ampliar automáticamente a AVM/semántica M3–M4, Git escritura/CI M5, plugins
externos post-M5 ni gestos de ratón sin admisión. El paquete de desarrollo y sus
hashes están en los metadatos Unicode. Sin CI, push o publicación.

Las secciones fechadas siguientes conservan decisiones/evidencia históricas;
sus antiguos «siguientes» y «pendientes» se subordinan a este estado vigente.


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

## M2B — GUI Windows mínima — EN CONSTRUCCIÓN; primer ejecutable validado

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


## Cierre CPU y siguiente bloque — 2026-11-02

CPU implementado y medido: [CPU_PERFORMANCE](CPU_PERFORMANCE.md). Search Plus usa
un solo engine por archivo, default dos trabajadores incluido coordinador,
configurable 1…4 con admisión/headroom conservadores. TUI ya acepta
`--search-workers 1..4`; uno reduce demanda mientras se trabaja con otras cargas.
Se conservan bytes/encoding/undo, revisiones, cancelación y cuotas globales; helpers
lazy reutilizados sin snapshots duplicados ni pools anidados/F3 documental ficticio.

Cuatro capturas / 1488 consultas conservadas, mismo probe/corpus y dos órdenes.
16 MiB: mediana 1,24–1,49× más rápida, uso agregado ~1→1,73–1,80 núcleos equivalentes;
CPU total aumenta 30–48%. Memoria privada muestreada del harness baja de
5,527–5,625 a 3,520–4,230 MiB, incluido reader sin crecimiento para guardia EOF.
Un caso pequeño tuvo mediana peor; no es aceleración universal ni menos CPU total.
Codegen jc-app s/2/3 se comparó aparte: se **mantiene s**, sin mejora UTF-8 consistente
que justifique cambiar política global. Qwen/carga, energía y temperatura no
observados/controlados; no tocar procesos ajenos. Pantalla física/global RAM abiertas.

Validación CPU: 269 pruebas distintas al completar las suites tras corregir dos
oráculos S1 sensibles a mayúsculas; luego integración de arranque TUI, 47 pruebas
TUI/CLI aprobadas con cinco nuevas, total **274 distintas**. Formato, Clippy
workspace/all-targets del executor y dirigidos posteriores, release y CLI real
pasaron. No una suite completa de 274 en una sola corrida. No repetir perfiles
ni suites por documentación; sin instalaciones/CI/push/afinidades/prioridades.

**Siguiente pendiente autorizado:** presentación TUI del estado Git de repositorio,
sobre el proveedor y controlador ya implementados en `16071e3`; leer
GIT_STATUS_READ_ONLY/GIT_STATUS_DELIVERY antes de conectar. Preservar paths byte
opacos, rechazo de filtros, configuración aislada, generación/opciones vigentes,
lectura visible y ausencia de escrituras. No prolongar optimizaciones CPU sin
otra pregunta concreta ni activar un incremento ajeno al plan.

## Cierre del siguiente incremento: Git status TUI — 2026-11-02

La presentación TUI pendiente en el cierre CPU ya está conectada al controlador
existente: consulta explícita de repositorio sin documento, rutas byte opacas,
filas visibles acotadas, selección, refresh/cancel y configuración con intención.
Entrega vigente solo por poll_tasks, composición de solo lectura y recuperación
tras pánico incluso obsoleto. No watcher ni rutas editables o escrituras Git.

Bloque integrado antes de validar: 290 pruebas completas aprobadas, fmt,
Clippy workspace all-targets y release bins/examples. [Contrato/uso y límites](GIT_STATUS_PRESENTATION.md)
y [evidencia](VALIDACION.md). CPU queda cerrado con sus datos existentes; no se
repiten perfiles ni pruebas por ajustes documentales. Las menciones anteriores
de presentación status pendiente describen checkpoints históricos. Navegación
segura desde paths Git, otras funciones M2 y aceptación global de RAM/pantalla
siguen pendientes; no se declara cierre de M2A ni se activa otro incremento aquí.

## Rectángulo de una acción y siguiente bloque activo — 2026-11-02

Implementado el [rectángulo acotado](M2_RECTANGULAR_SELECTION.md): selección real
por celdas, TAB4/virtual, clipboard interno rectangular, copy/cut/paste/entrada/
Delete/Backspace con transacción única, comandos y Palette, paint readonly.
No es cierre del modo columna Unicode general: rechaza grafemas/anchos no
admitidos; las ediciones efectivas terminan el rectángulo. No OS clipboard ni GUI.

Cierre local: 310 pruebas distintas aprobadas en cierre selectivo después de
corregir un acceso que exponía también selección lineal; fmt/Clippy/release.
No repetir pruebas aprobadas ni perfiles por documentación. El siguiente pendiente
M2A está activo: revisión/contrato de unir y dividir líneas genéricos del anexo
ABL_EDITOR_COMMAND_SCOPE. Join usará rangos de separadores/indentación y un batch
F1; Split reutilizará insert_newline/preferred EOL. No ampliar a AVM/GUI, Unicode
general ni otra optimización CPU sin una necesidad concreta.

## Join/Split cerrados y siguiente admisión activa — 2026-11-02

Completado [unir/dividir líneas](M2_LINE_COMMANDS.md): modelo universal,
transacción única, lectura de prefijos/EOL por rangos y clasificación byte→línea
sin layout escalar. Split reutiliza la inserción de newline existente; Enter se
centraliza y Ctrl+Shift+J no interfiere con Ctrl+J. Preservados BOM/EOL final,
encoding/bytes fuera de cortes, vistas y undo. No normalización sintáctica.

Validación: 74 dirigidas de app y cobertura TUI completada tras corregir solo un
fixture de foco; nueve nuevas, 319 distintas al cierre, fmt/Clippy/release.
No una suite única319 ni una repetición de perfiles. Contrato TUI se reconcilia
con las funciones implementadas y límites actuales, sin inferir recorrido físico.

**Siguiente bloque activo:** auditoría/admisión de regex real acotado de Search
Plus, prioridad universal pendiente de M2A. Revisar engine compartido y contrato
de bytes/Unicode, presupuesto/cancelación, capturas/reemplazo y costo sin instalar
dependencias. No habilitar el switch hasta que haya un motor operativo y validado;
no adelantar proveedores de extensión/AVM M3 ni GUI sin ventana autorizada.

## Admisión regex completada: decisión de dependencia pendiente

La auditoría siguiente ya está recogida en [M2_REGEX_ADMISSION](M2_REGEX_ADMISSION.md).
No se habilitó regex ni se descargó nada. Candidato para evaluar: regex-automata
0.4.18 mantenido, con compilación/VM cooperativa, presupuestos, coordenadas y
reemplazos explícitos; no asumir que una llamada monolítica es cancelable.
Lock/cache no contienen un motor. La instrucción de continuidad «sin instalar
dependencias» exige decisión de Juan antes de añadir/fijar esa dependencia Rust.
Alternativa: conservar regex deshabilitado y seleccionar otro pendiente M2A.
No implementar un parser/engine casero para esquivar la restricción. La admisión
es diseño/revisión de fuentes primarias y código local, sin pruebas/benchmarks
nuevos ni aceptación de un motor; siguen vigentes los 319 casos del cierre previo.

## Prioridad vigente de Juan: construir M2A y M2B — 2026-11-02

Juan cerró el plan correctivo y pidió continuar el roadmap original: «Continúa
con la construcción, síguete con M2A y M2B»; aclaró que automatizamos hasta donde
sea posible y solo deja las pruebas de usuario para el final. Las puertas globales
RAM/pantalla quedan documentadas, sin bloquear construir funciones ni abrir
optimizaciones indefinidas. Regex permanece pendiente sin autorización para
agregar/descargar/fijar regex-automata u otra dependencia; no volver a pedirlo para
avanzar un pendiente independiente.

Frentes admitidos en el árbol actual: [Quick Open M2A](M2_QUICK_OPEN.md), reservado
Ctrl+P del roadmap, y [primer GUI Windows M2B](M2B_NATIVE_WINDOWS.md) mediante
Win32/GDI nativo y Workbench compartido. Dependencias del GUI: solo paquetes
existentes jc-app/jc-text, sin descarga de framework. El HTML de prototypes sigue
siendo estudio histórico, no el ejecutable. Un integrador, hasta tres agentes
totales, archivos exclusivos y validación pesada serial al cerrar bloques amplios.
No CI/push/instalaciones, cambios de seguridad/servicios ni Qwen/GPU.

Estado al activar: implementaciones en construcción/integración, sin anunciar aún
pruebas de este bloque. El GUI real tendrá smoke de HWND propio oculto y renderer
GDI; no controla otras aplicaciones. Validar compilación, comportamiento compartido,
reporte/bitmap y límites, y dejar checklist manual para Juan sin pedir prueba ahora.

## Primer cierre funcional M2A+M2B y siguiente bloque activo

Quick Open de proyecto TUI y GUI Win32/GDI inicial implementados: consulta
cancelable/acotada, revalidación/apertura confinada y sesión única; ventana real
con edición/selección/undo/grupos/guardar y frame readonly. [Uso/límites finder](M2_QUICK_OPEN.md)
y [GUI/evidencia nativa](M2B_NATIVE_WINDOWS.md).339 pruebas Rust distintas en
cierre selectivo +16 checks del ejecutable real, fmt/Clippy/release aprobados.
No cambios de dependencias externas, CI/push o perfiles repetidos.

M2B continúa en construcción: no declarar paridad completa ni puertas físicas
cerradas por ese smoke. El siguiente bloque ya admitido/construyéndose es adaptar
Quick Open y Command Palette al GUI, con selección/input acotados y presentación
GDI readonly, usando los controllers y comandos existentes. Una notificación al
loop nativo debe recibir completions sin otra tecla y sin polling permanente.
Regex permanece pendiente; no solicitar su dependencia para avanzar este frente.
Las pruebas manuales de usuario se conservan para el final.


## Cierre de selectores GUI y siguiente construcción M2B

Quick Open/Palette/Open Folder nativos implementados: [contrato](M2B_NATIVE_SELECTORS.md).
350 pruebas completas locales y23 checks del release real, Clippy/release aprobados.
Se conserva el único Workbench/undo y no hay hilo puente/indexador ni dependencia
nueva. M2B permanece abierto. Próximo bloque amplio: clipboard OS + elección de
encoding en GUI; pruebas locales al cerrar ambas rutas. Manual/IME/DPI/accesibilidad
y aceptación física/global siguen pendientes, sin posponer construcción funcional.


## Prioridad de las tres funciones — cierre funcional

Juan pidió construir juntas Mostrar invisibles, buscar bloques multilínea y
reemplazarlos, antes de nuevas superficies. Las tres están integradas en TUI/GUI:
[contrato, uso y evidencia](M2_INVISIBLES_MULTILINE.md). Mapa ASCII por spans fuente,
caret/selección/scroll propios de display, EOL exactos y campos navegables/pegables
con gestos distintos de ejecutar. Replace-all documental conserva una transacción.
También se integraron clipboard lineal nativo y elección gráfica de interpretación.

376 Rust distintos en cierre selectivo; Clippy/release y29 checks nativos pasaron.
No se infiere aceptación OS clipboard/IME/DPI/accesibilidad/pantalla física ni RAM
global. Regex/dependencias siguen aplazadas. Próximo pendiente funcional M2B:
flujo Save All explícito por documento, incluyendo Save As para nuevos y cancelación;
nunca guardar nombres untitled generados por un shortcut. Validar al cerrar ese
bloque, sin repetir las pruebas de este cierre por documentación.


## Save All GUI implementado; siguiente S1 gráfico

[Save All nativo](M2B_SAVE_ALL.md) ya resuelve nuevos con Save As/cancelación,
targets por revisión/path y resultados parciales explícitos, sin mover vistas.
6 pruebas nuevas,45 dirigidas y32 checks nativos aprobados; conjunto histórico382
distintos, Clippy/release aprobados. Las tres funciones prioritarias anteriores
siguen implementadas. Próximo bloque amplio M2B: Search Plus de proyecto en GUI
con controller S1 existente, entrada acotada y resultados versionados/solo lectura.
No nuevo motor/índice permanente, regex ni dependencies/CI. Construir antes de
otra ventana de tests; manual/aceptación global permanece abierta.

## CIDE y Search Plus de proyecto GUI — cierre 2026-11-02

Marca visible CIDE; comandos cide/cide-tui/cide-gui con aliases jc-ide compatibles.
Crates, raíz, remoto e historia no se renombran. Icono original JG+CIDE verde para
TUI/CLI; variante naranja para GUI, recursos PE de siete tamaños y HWND verificados.
[Inventario/contrato](CIDE_REBRAND.md).

[S1 gráfico](M2B_PROJECT_SEARCH_NATIVE.md) implementa campos Query/Include/Excludes,
worker configurable1..4, resultados acotados por notificación, apertura validada,
refresh, stale/cancel y cambio de raíz. Misma lógica S1; sin índice/motor nuevo.
391 casos distintos aprobaron por suites (260+131), fmt/Clippy/release y41 checks
nativos. ZIP local de desarrollo con manifiesto, sin instalación/publicación.
Quedan los flujos GUI de proyecto replace y diff/status read-only, proveedores y
puertas físicas/globales ya documentadas. No se cierra M2A/M2B ni se solicita prueba
manual intermedia; no repetir suites/perfiles aprobados para cambios documentales.

## Reemplazo y Git GUI integrados — 2026-11-03

Bloque amplio según instrucción nocturna de Juan: [reemplazo y Git readonly](M2B_PROJECT_REPLACE_GIT.md)
construidos/cableados antes del cierre integral. Entrada multilínea/preview,
confirmación, un archivo por turno, no-op, undo, conflictos/cancelación y resumen;
status de disco/index y diff HEAD/índice contra buffer sucio, navegación validada,
configuración explícita, delivery y pintura sin admisión de snapshots.

53 Rust distintos dirigidos y58 checks nativos aprobaron;8 nuevos, histórico399
(no corrida global399). fmt/Clippy/release aprobados, metadataGit sintética intacta.
Sin dependencias/CI/push ni perfiles repetidos. M2A/M2B y gates físicos/globales
permanecen abiertos. Próximo frente M2B activado: contrato del Explorer lateral
gráfico sobre catálogo lazy compartido/acotado; respetar raíz, dirty, pin y vistas.
Construcción completa antes de su siguiente ventana integral, sin pedir pruebas
manuales al propietario ni ampliar a servicios/regex/dependencias no autorizados.

El [contrato Explorer](M2B_EXPLORER_CONTRACT.md) ya inventaría la ruta TUI síncrona
y define catálogo único, identidades, cuotas y migración de ambos frontends. Es
diseño previo; el cierre de implementación se registra a continuación.

## Explorer TUI/GUI compartido — 2026-11-03

[Implementado](M2B_SHARED_EXPLORER.md): catálogo único por sesión con IDs opacos,
worker lazy, cuotas globales, cancelación/generaciones; vista compartida y apertura
confinada con dirty/pin/side/undo. Se retiró la enumeración y apertura alternativa
TUI. GUI tiene sidebar persistente, mouse/teclado, menú/palette, foco y modo compacto
que conserva split. Windows enumera por handle con ancestros fijados; fallback no
Windows mantiene ventana de namespace explícita, sin aceptación local.

Bloque completo construido antes de pruebas:352 casos Rust relacionados aprobados
(13 nuevos; un test del proveedor eliminado retirado),70 checks GUI y seis flujos
ConPTY reales. fmt/Clippy/release aprobados; fuentes y metadataGit propias intactas.
Evidencia cruda y hashes en M2B_SHARED_EXPLORER. Sin dependencias nuevas/CI/push.
M2A/M2B y gates globales/manuales permanecen abiertos. Continuación: completar la
paridad funcional GUI pendiente y preparar el API opcional de proveedores de
lenguaje según EXTENSION_ARCHITECTURE, sin asumir parser/LSP/ABL ya disponibles.

## Paridad de navegación GUI — 2026-11-03

[Bloque integrado](M2B_NATIVE_NAVIGATION.md): Ctrl+G/línea-columna escalar con campo
acotado, aceptación explícita y TaskStamp+ViewId/pane; atrás/adelante por grupo y
historial de consultas/options en Find/Replace. Menú/atajos/paleta comparten registro
y Workbench. Parser de posición compartido con TUI; cancelación de F3 diferido sin
parar el scan, borrador de Replace conservado a través de la paleta. Pin mantiene
su regla existente: Go to se dirige al otro grupo; back/forward usa el activo.

78 casos dirigidos aprobados (cuatro TUI nuevos),14 checks nativos de sección
navigation (siete nuevos), fmt/Clippy/release aprobados. El único fallo inicial fue
el oráculo del pin, corregido sin cambiar el modelo. No se repitieron suites
Explorer, recorrido nativo global ni perfiles cerrados. Evidencia cruda y hashes
retenidos; sin dependencias/CI/push ni pruebas manuales solicitadas.

Siguiente paridad GUI pendiente: edición universal de líneas y comentarios ya
disponible en jc-app/perfiles (duplicar/eliminar/mover y delimitadores del perfil),
con foco y registro compartidos antes de proveedores especializados. No ofrecer
handlers sin cablear. M2A/M2B y las puertas físicas/globales siguen abiertos.

## Paridad GUI de líneas y comentarios — 2026-11-03

[Bloque implementado](M2B_NATIVE_LINE_EDIT.md): seis comandos universales ya
cableados a teclado/menú Edit/paleta, Workbench y perfiles existentes. Disponibilidad
central requiere editor_input/foco/selección lineal/capacidad del lenguaje; captura
de origen de paleta rechaza documentos, revisiones, vistas y selecciones cambiados.
Sin segunda implementación de edición ni cambios al motor o semántica TUI.

Construcción amplia primero;23 casos dirigidos y14 checks nativos de sección
line-edit aprobaron a la primera (3/7 nuevos), fmt/Clippy/release aprobados.
Cuatro encodings, BOM/EOL, undo/redo, split/pin y modified limpio→sucio→limpio sin
Save observados. No repetir navegación/Explorer ni perfiles cerrados para docs.
Sigue pendiente la paridad de comandos universales restantes del registro en GUI
(p. ej. búsqueda sobre selección y reapertura de pestaña cerrada); luego API de
proveedores opcionales conforme al plan. No parser/AVM/CI nuevo; gates globales abiertos.

## Selección literal y ciclo de pestañas GUI — 2026-11-03

[Implementado](M2B_SELECTION_REOPEN.md): selección next/previous, reapertura,
cancelación explícita y posiciones de pestaña por menú/atajo/paleta sobre comandos
existentes. Source-selection exige foco/selección lineal y target actual; errores
de decode/cuota no sustituyen la consulta. Reopen propaga IO y conserva historial;
rechaza ruta ya abierta para impedir otro buffer. Cierre dirty confirma el mismo
doc/rev/vista/intención tras modal; solo la última vista crea metadata de cierre.

11 casos dirigidos (cuatro nuevos) y15 checks HWND de sección (ocho nuevos)
aprobados; fmt/Clippy/release aprobados. Fallos iniciales de montaje diferido/getter
privado retenidos, sin repetir otros casos aprobados ni suites cerradas.
Seguimiento M2B: capacidades universales GUI restantes de selección/rectángulo y
presentación según contratos ya existentes, antes de proveedores opcionales.
M2A/M2B y gates físicos/globales abiertos; no AVM/dependencias/CI/push nuevos.

## Selección rectangular GUI y presentación — 2026-11-03

[Bloque integrado](M2B_RECTANGLE_GUI.md): seis comandos/menú/paleta/AltShiftflechas,
Alt+drag con captura versionada, Source geometry absoluta compartida, clipboard
interno/edición/undo y cuadrícula TAB4 también con invisibles en GUI/TUI. No motor
rectangular nuevo ni ampliación de repertorio/cuotas. Se retiró la ruta visual
rectangular de escapes que confundía columnas; al cancelar retorna diagnóstico.

26 casos dirigidos (cuatro nuevos) y15 checks HWND (ocho nuevos), fmt/Clippy/release
aprobados. Dos bugs corregidos con evidencia: permiso TUI tras Find y salida de
rectángulo en BOF/EOF sin byte adyacente. Fallos y hashes retenidos, Source13B intacto.
No suites cerradas/perfiles/CI repetidos. Queda inventario final de paridad universal
GUI y puertas M2 físicas/globales; proveedores opcionales según arquitectura después.
Sin nuevas dependencias/instalación/AVM/push ni claim de columna Unicode general.

## Inventario universal GUI y edición/grupos — 2026-11-03

[Inventario completo](M2B_GUI_INVENTORY.md) y matriz con104 CommandId, entrada,
predicado/foco, ruta real y evidencia histórica acotada.85 con rutaGUI,19 sin
supported por dependencia/capacidad pendiente o adaptación Menu* de Win32.
No equivalen a104 funciones aceptadas ni a todos sus caminos probados.

Huecos independientes encontrados y cerrados juntos: Indent/Outdent ahora en
menúEdit; Link/UnlinkScroll en View/tecla/paleta e indicador persistente. Cuatro
ediciones (Indent/Outdent/Join/Split) exigen editor_input, también enTUI. Modelo
Workbench de bytes/vistas/undo/delta scroll conservado, sin proveedor nuevo.
Dos casosRust dirigidos (uno nuevo),12 checksHWND (cinco nuevos),fmt/Clippy/release
aprobados tras integrar todo; fallo inicialClippy conservado, sin suites cerradas.

Próximo bloque universal concreto: hostGUI de panel inferior, Problems/Output
F3 y foco de tres regiones. Concretar contrato de filas/destinos/cancelación y
estado antes de construir; esto no autoriza inventar proveedores. SearchResults
tiene panel de proyecto específico, no ese host. MenúsTools/Help, entrada avanzada
en selectores, selección lineal por drag/wheel y resto físico/global siguen
identificados, sin afirmar paridad completa. No CI/push/dependencias/AVM nuevos.

## Host GUI de paneles F3 y foco — 2026-11-03

[Bloque integrado](M2B_BOTTOM_PANELS.md): Problems/Output/ToggleBottomPanel en
View/paleta/teclado, selección por IDs, navegación validada y foco en tres regiones.
TaskPanels reemplaza logs de strings y conversión duplicada TUI, conservando una
sola copia de texto por fila y procedencia tipada; GUI acepta F3 en un único poll.
JobSystem captura ruta para invalidar SaveAs aun con revisión/encoding iguales.
Preservados límites F3, cuotas de log, fuente/vistas/undo, pin y render readonly.
No proveedor nuevo: productores sintéticos solo en automatización; vacío explícito.

23 casos dirigidos distintos (cuatro nuevos),13 checks HWND (seis nuevos),
fmt/Clippy/release aprobados tras cableado completo. Dos corridas nativas de la
misma sección: final incluye procedencia visible; no suites antiguas sin impacto,
mediciones globales o CI. SaveAs de memdoc en destino propio nuevo prueba identidad;
archivo exacto verificado/eliminado, fuente13B y metadata Git sintética intactos.

Ahora88 de104 comandos tienen ruta GUI; no aceptación de todos sus caminos.
Pendientes universales concretos del inventario: entrada avanzada/pegado de
selectores, adaptación incompleta de menús y selección lineal por drag/wheel.
Proveedores internos/terminal/regex requieren sus propios contratos/puertas; no
inferir implementaciones de negocio del host. Manual/global/Unicode/clipboard OS,
symlink y notices GNU siguen abiertos. Sin dependencias/proveedores/AVM/CI/push nuevos.

## Entrada avanzada de selectores GUI — 2026-11-03

[Bloque integrado](M2B_SELECTOR_INPUT.md) en Quick Open/paleta/encoding: campo único
con caret/anchor, Shift selección, Home/End/Delete, Ctrl+A/C/X/V/U y rutas de menú
Edit, sin confundir edición de campo con comandos del invocador de paleta. Up/Down/
Page/Tab navegan candidatos; CtrlHome/End primero/último. Cuota256UTF8 y rechazo
atómico de controles/bidi/overflow; handoff externo fuera de préstamos, revalidado
por generación/intención/candidato/doc/vista/encoding/ruta/raíz/foco. Solo transporte
sintético en pruebas; no clipboard privado leído/alterado durante desarrollo.

FieldSelection reutiliza edit_field y un walker de presentación común; retiene
solo filas visibles del campo (dos pasadas <=4096B), sin otra historia/motor fuente.
23 casos Rust distintos aprobados (24 pases con un solapamiento de filtro), cuatro
nuevos;13 checks HWND (seis nuevos), fmt/Clippy/release. Fallos de expectativa/
getter privado/enum grande retenidos y corregidos; native aprobó primera corrida.

Continúan88 rutasGUI, sin reclamar104 funciones aceptadas. Pendientes universales
del inventario: adaptación de menús nativos y selección lineal por drag/wheel.
Proveedores/terminal/regex, aceptación física/global/Unicode/clipboardOS, symlink
y notices GNU conservan sus puertas. Sin nuevas dependencias/AVM/CI/push/publicación.

## Menús nativos y gestos de fuente — 2026-11-03

[Bloque integrado](M2B_NATIVE_INTERACTION.md) posterior a `ae9a813`: menús desde
el modelo compartido y handlers reales, sin rangos numéricos. Seis macros de
paleta y Alt/F10 abren HMENU; elección revalidada por contexto/capability/origen,
atajos/checks según control activo. Sin contenido inventado para Build/Help.
Ahora94 rutasGUI de104 IDs históricos:88 acciones y seis macros, sin certificar
todos sus caminos físicos ni alterar el inventario histórico.

Drag lineal/Shift mantiene ancla/grupo, convierte proyección a bytes legales y
rechaza destinos obsoletos. Captura común con rectángulo fuera del préstamo.
Wheel fraccionaria signed sobre el grupo bajo el puntero conserva foco/caret/
selección; viewport manual por vista y enlace por delta, pin independiente.
Se eliminan la antigua ruta click mutable y el constructor de menús numérico.

Construcción completa antes de pruebas:16 Rust distintos (cuatro nuevos),13 HWND
(seis nuevos), fmt/Clippy/release aprobados. Fallo inicial de tres lints de arnés
retenido, native a la primera; ZIP local verificado. Sin suites cerradas repetidas
sin impacto, perfiles globales o CI. Estos dos huecos quedan cerrados localmente;
pantalla/ratón/teclado físicos, clipboard OS/IME/DPI/accesibilidad, Unicode general,
latencia/RAM global, mouse horizontal/autoscroll/word-selection/campos y symlink/
notices siguen pendientes. Proveedores/terminal/regex conservan contratos y puertas
propias. No inferir M3 ni distribución certificada. Continuar en bloques amplios
y dejar pruebas manuales/OS con el usuario al final, según su preferencia.

## Seguimiento de validación y límite de admisión — 2026-11-03

[Auditoría previa y evidencia](M2B_MENU_INVOKER_VALIDATION.md), base171af05.
Se cierra el recorrido comprometido paleta→macro de menú con Find/Replace:
seis macros, tres estados de campo, cancelación repetida, continuidad del teclado,
acción View permitida, edición fuente rechazada y ocho cambios tardíos de draft.
Un check integrado nuevo + siete de arranque/pintura, tres rechazos CLI y fmt/
ClippyGUI/releaseGUI aprobados a la primera. Sin pruebas unitarias, repetición de
la sección histórica13 o cambios de handlers; no nueva funcionalidad de producto.

La auditoría de la mención previa a mouse horizontal/autoscroll/word-selection/
campos no la convierte en admisión: son limitaciones/exclusiones, sin contrato
específico de siguiente bloque. Su construcción no se activa. Hace falta decidir
alcance, coordenadas, límites y cancelación del incremento antes de implementarlo.
Esta nota registra la decisión de no ampliar, sin añadir funcionalidades al plan
para justificar código retrospectivamente. M3/proveedores y puertas físicas/globales
no se infieren ni se cierran; sin dependencias/AVM/CI/push/publicación nuevos.

## Cierre local — resaltado compartido, 2026-11-03

Por prioridad explícita de Juan se integró resaltado léxico ABL, Rust, JSON y
JSONC en TUI y GUI; otros perfiles permanecen Plain. Contrato, límites, fallos
iniciales y evidencia: [M2_SYNTAX_HIGHLIGHTING.md](M2_SYNTAX_HIGHLIGHTING.md).
Pasaron35 casos Rust distintos (18 nuevos),11 checks HWND (4 nuevos),5 flujos
ConPTY,3 rechazos CLI, fmt, Clippy y build release locales offline. No se ejecutó
toda la suite runtime del workspace ni CI. No equivale a aceptación física/global.

El diff entre documentos abiertos queda suspendido y recuperable en
`checkpoints/open-diff-before-syntax-66aa783.zip` y su manifiesto; no está activo
ni validado. No restaurarlo automáticamente sobre el resaltado.

La autorización posterior del usuario permite incorporar y fijar una dependencia
Rust mantenida para regex real (candidato regex-automata) en el siguiente bloque
Search Plus. Sustituye la restricción anterior para esta dependencia concreta:
no pedir de nuevo permiso. No se añadió en este bloque, regex sigue deshabilitado
y no están autorizadas instalaciones de programas del sistema.

## Regex implementado — continuación TUI, 2026-11-03

Regex real de documento y proyecto está integrado con regex-automata=0.4.18,
autorizado por Juan. Alt+R, historial, rangos originales, cancelación/cuotas y
reemplazo literal por transacciones; [contrato y evidencia](M2_REGEX_SEARCH.md).
Esta actualización sustituye las menciones históricas de regex deshabilitado.
124 casos distintos aprobados observados, cuatro flujos ConPTY y fmt/Clippy/
release locales. Dos fixtures junction y seis Git fallaron por creación/acceso
restringido: no se declara suite global aprobada ni aceptación física/RAM.

La instrucción vigente de Juan es continuar hasta terminar los pendientes TUI,
en bloques amplios, validar al cerrar cada bloque y activar el siguiente sin
esperar otro «continúa». Siguiente bloque: comparación de documentos abiertos
sin Git, desde el checkpoint revisado sobre el código actual. No restaurar fuentes
antiguas sobre resaltado/regex. Proveedores ABL/AVM y aceptación física conservan
sus requisitos reales; no afirmar capacidades que solo tengan contrato.

## Comparación de abiertos TUI implementada — 2026-11-03

El checkpoint suspendido de comparación sin Git ya se integró sobre regex y
resaltado actuales: controller con dos orígenes versionados, admisión dual F3,
presentación compartida acotada y visor TUI lateral/unificado. Comandos View/
paleta, hunks, navegación a grupo original, stale/refresh/cancel; fuente de solo
lectura y sin writes. [Contrato y evidencia](M2_OPEN_DOCUMENT_DIFF.md).
GUI de este visor sigue pendiente; no restaurar su checkpoint automáticamente.

54 casos distintos observados aprobados, cuatro flujos ConPTY, fmt/Clippy/release.
Las nueve regresiones Git pasaron con acceso autorizado a fixtures propios tras
fallar por acceso sandbox; no se usa Git en la comparación del producto.
No CI ni aceptación física/global. Siguiente bloque realmente activo: contrato
de capacidades internas opcionales y consumidor TUI de símbolos/navegación,
según EXTENSION_ARCHITECTURE y [matriz de cierre TUI](M2_TUI_FINISH.md).

## Capacidades locales de fuente TUI — 2026-11-03

Primer registro interno namespaced y proveedor opcional progress.abl operativo
para símbolos y referencias léxicas; core.words completa palabras textuales de
cualquier documento. Listas/selector TUI, entrega F3, stale, candidatos confinados
y propuestas de completado por revisión/una transacción. [Contrato y evidencia](M2_SOURCE_SERVICES.md).
40 casos distintos (17 nuevos), cuatro flujos ConPTY, fmt/Clippy/release locales
aprobados. No AVM, semántica, loader externo, GUI ni aceptación física/global.

Siguiente bloque activo: plantillas/snippets universales y catálogo ABL del
proveedor opcional, parámetros/preview/aceptación y undo único. Continuar sin
esperar otro «continúa»; construir el conjunto antes de su ventana de validación.

## Snippets TUI — cierre local 2026-11-03

Motor universal acotado con parámetros, preview y aceptación por revisión/una
transacción. progress.abl aporta siete plantillas declarativas de Insert/Surround;
sin reglas ABL en jc-app. Preserva encoding/BOM/EOL/undo; propuestas obsoletas se
rechazan. [Uso y evidencia](M2_SNIPPETS.md):38 casos distintos (11 nuevos), cuatro
recorridos ConPTY, fmt/Clippy/release aprobados. Sin AVM ni aceptación física/global.
Sigue auditoría final de teclado, foco y flujos TUI; CI permanece en M5.

## Auditoría de overlays/teclado TUI — 2026-11-03

Pegado y teclas respetan el overlay superior sobre snippets/completado/diff.
Rutas/prompts4096 UTF8, posición64 y paleta256; rechazo atómico al exceder cuota.
CtrlU vacía campos; Home/End/Page navegan menú/paleta sin perder selección visible,
Page en preview de reemplazo usa filas de la última presentación. Sin segundo buffer.

117 casos TUI distintos aprobados (cinco nuevos), entre corrida y cierre dirigido.
Dos expectativas históricas se actualizaron a regex habilitado y al nuevo final
del menú View; fixture de menú Help vacío corregido. Fmt/Clippy/release aprobaron.
Tres flujos ConPTY finales aprobaron,27557 bytes VT/salida0/fixture intacto.
Tres intentos iniciales quedaron esperando el título de la paleta, confundido con
una entrada del menú por el borde del snippet; se corrigió ese oráculo. El primer
intento nunca llegó a enviar bracketed paste: no acredita ni refuta ese transporte.
Event::Paste y sus cuotas sí están cubiertos por casos Rust; no clipboard OS real.

Siguiente bloque activo: adaptar clipboard Windows estricto a la TUI, solo por
acción explícita; separar validación inyectada de interoperabilidad física.

## Clipboard Windows TUI — cierre local 2026-11-03

Adaptador CF_UNICODETEXT de la GUI extraído a jc-platform (sin dependencias
externas); ambos frontends comparten implementación. TUI conecta transporte en
el loop real y crea HWND propio invisible bajo demanda, solo por Copy/Cut/Paste.
Headless conserva clipboard interno; pruebas inyectan memoria, no el clipboard
personal. Cortar después de publicar, pegar por destino/revisión y un undo;
cuotas/encoding estricto, campos del overlay superior y rectángulos internos.

32 casos Rust distintos aprobaron: cinco de plataforma (cuatro migrados y
uno nuevo de HWND propio), siete TUI nuevos y regresiones de clipboard/F1/registro.
Fmt, Clippy workspace/all-targets y release bins/examples aprobaron offline.
GUI HWND propio:13 checks selector-input, GDI sin error, fixture y metadata Git
intactos, clipboard sintético. ConPTY TUI: tres flujos,27436 bytes VT/salida0,
fixture intacto. No lectura/escritura de clipboard personal ni certificación OS.
Un fixture usó un nombre de método inexistente y un campo tenía visibilidad más
amplia que su trait; se corrigieron antes del cierre, logs retenidos.

[Contrato y límites](M2_TUI_CLIPBOARD.md). Sigue layout Unicode general: Juan
autorizó unicode-segmentation=1.13.3 y unicode-width=0.2.2; [admisión](M2_UNICODE_LAYOUT_ADMISSION.md).
Esa autorización sustituye la restricción previa para estas dos dependencias;
no pedirla de nuevo. Sin instalación de programas, CI ni AVM.
## Enlaces, esquema y renombrado

La navegación lateral de enlaces Markdown y el Esquema ABL de la GUI están
integrados. El proveedor ABL incorpora renombrado de variables de documento
y locales de procedimiento o función en un subconjunto comprobable, con
vista previa y una transacción. También admite declaraciones y llamadas
estáticas a procedimientos y funciones internas en el mismo archivo, incluidos
los pares de prototipo `FORWARD` e implementación con firmas compatibles. Sigue
ampliar los contextos y la resolución entre archivos.
Ver [alcance y orden de implementación](ENLACES_ESQUEMA_RENOMBRADO.md).

La [navegación por proveedor](NAVEGACION_MULTILENGUAJE.md) separa llamadas ABL
de inclusiones y conecta el Esquema y las dependencias de JavaScript.
**Llamado por** incorpora navegación inversa directa desde fuentes en disco
para ambos proveedores, con apertura lateral y revalidación antes del salto.

## Expansión DataDigger

Planificado como navegador de registros de solo lectura en una cuadrícula
nativa de CIDE, con un auxiliar ABL y verificación independiente en OpenEdge
10.0B y 10.2B. Ver [plan de expansión](EXPANSION_DATADIGGER.md).
