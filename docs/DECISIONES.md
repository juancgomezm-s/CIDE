# Registro de decisiones

## D01 — Núcleo de edición independiente

**Aceptada.** Rust se usa para el núcleo inicial. Documentos y operaciones no dependen de widgets ni de una sesión de Progress.

## D02 — Bytes como representación canónica

**Aceptada.** Los fuentes no se pueden asumir UTF-8. El almacenamiento conserva bytes y las conversiones textuales son explícitas.

## D03 — Reutilización selectiva de Lime

**Aceptada.** Solo se portan comportamientos pequeños, probables y atribuibles. No se porta el editor completo ni su compatibilidad Python/Sublime.

## D04 — Progress como proveedor de compilación

**Aceptada.** La instalación seleccionada por proyecto es la autoridad para sintaxis, compilación y resolución estática de esquema.

## D05 — Catálogo local de esquema

**Aceptada.** La navegación y el completado consultarán metadatos locales actualizables sin mantener obligatoriamente una conexión permanente.

## D06 — Servicios opcionales

**Aceptada.** Progress, Git, LSP, terminal y agentes se inician bajo demanda y no forman parte del consumo basal.

## D07 — Interfaz Windows

**Pendiente.** Se medirá interfaz/renderizado propio frente a componentes existentes antes de comprometer la arquitectura gráfica.

## D08 — Integración con OmniCore

**Diferida.** OmniCore conserva su núcleo C# y contratos propios. JC-IDE podrá intercambiar revisiones y cambios sin fusionar los dos historiales de undo.

## D09 — Multilenguaje desde la base

**Aceptada.** ABL tiene prioridad de integración pero no exclusividad. Los proveedores de lenguaje permanecen fuera del núcleo de bytes.

## D10 — TUI y GUI comparten modelo de aplicación

**Aceptada.** Se crea `jc-app` como capa sin UI con `CommandRegistry`, `KeyMap`, `FocusManager`, menús, pestañas, paneles y diálogos.

Reglas vinculantes:

- ningún widget define su propia combinación de teclas para una acción de negocio;
- menú y shortcut consultan el mismo `CommandId`;
- los bindings contextuales pueden sombrear uno global únicamente por scope explícito;
- `Esc` cierra primero overlays y después devuelve el foco al editor;
- `F6`/Shift+F6 recorren solo áreas primarias visibles;
- `Tab` simple no cambia de área ni pestaña;
- cerrar una pestaña sucia exige una decisión explícita;
- comandos Progress permanecen deshabilitados fuera de contexto;
- TUI y GUI deben consumir este modelo común, no duplicarlo.

## D11 — Encoding legacy explícito, nunca inferido destructivamente

**Aceptada en M1.** UTF-8 puede identificarse por BOM o por una secuencia válida; ASCII se marca como compatible pero ambiguo. Si los bytes no son UTF-8, JC-IDE no decide entre Windows-1252, ISO-8859-1, IBM850 u otra code page por frecuencia de caracteres.

La code page real pertenece al perfil del proyecto/documento. Los bytes no interpretables permanecen en el documento y se reportan como incidencias. La inserción de texto es estricta: si el encoding seleccionado no representa un carácter, la edición falla antes de modificar el buffer.

## D12 — Guardado con conflicto explícito y reemplazo atómico

**Aceptada en M1.** La sesión conserva una huella de contenido de la versión abierta/guardada. Antes de un Save normal compara el disco y rechaza cambios o eliminación externos.

El contenido se escribe a un temporal del mismo directorio, se sincroniza y después se publica. Archivos existentes usan reemplazo atómico por plataforma; archivos nuevos usan creación no-clobber. `force_save` es la única ruta del núcleo que permite sobrescribir deliberadamente una versión externa.


## D13 — CI diferido hasta M5

**Aceptada.** GitHub Actions no forma parte del ciclo de construcción de M2, M3 o M4. Ejecutar CI completo después de cada cambio consume recursos y obliga a estabilizar repetidamente interfaces que todavía están evolucionando.

Hasta M5:

- los workflows no se disparan por `push` ni `pull_request`;
- el workflow existente queda disponible solo mediante `workflow_dispatch` y no se invoca sin autorización explícita del propietario;
- los agentes no deben reactivar CI para resolver errores intermedios;
- la validación se hace local y de forma proporcional al cambio cuando el toolchain esté disponible;
- si un entorno no permite compilar/probar, se documenta como pendiente en vez de usar CI remoto como sustituto.

En M5 se realiza el hardening transversal y se vuelve a habilitar la matriz automática Windows/Linux una vez que las capas principales hayan dejado de cambiar estructuralmente.


## D14 — Compatibilidad Sublime solo después de M5

**Aceptada.** El soporte de plugins, extensiones y paquetes de Sublime Text se implementará únicamente después de cerrar M5.

No se incorporará una runtime Python ni una capa de compatibilidad Sublime durante M2–M5. El trabajo post-M5 debe mantenerse como adaptador sobre `jc-app`, `jc-languages` y los frontends, sin introducir dependencias de Sublime en `jc-core`.

La compatibilidad será por niveles y con capacidades explícitas. Los recursos declarativos se priorizan sobre la ejecución de código; los plugins Python requerirán sandbox, permisos y una API de compatibilidad limitada y versionada.

## D15 — Productividad ABL y Search Plus

**Aceptada.** JC-IDE no reproducirá todos los asistentes PDSOE. Se separan comandos de edición local, plantillas/lexing por versión y análisis semántico; las capacidades caras se cargan bajo demanda.

Atajos de edición: Ctrl+D duplicar, Ctrl+Shift+D borrar línea, Tab/Shift+Tab sangría, Ctrl+/ comentar/descomentar y Ctrl+Shift+/ bloque cuando el lenguaje lo soporta. ABL usa por defecto /* ... */. La selección de columna requiere semántica rectangular real; no basta una selección visual falsa.

**Search Plus** tendrá prioridad funcional sobre los asistentes OO complejos. Búsqueda en proyecto cancelable, streaming y resultados tipados; índice y agentes opcionales. Los cambios masivos requieren vista previa y control de versiones de archivos.

La política D13 sigue vigente: no consumir CI remoto antes de M5 salvo autorización explícita.

Documentación: docs/ABL_EDITOR_COMMAND_SCOPE.md y docs/M2_SEARCH_PLUS.md.

## D16 — Extensiones especializadas sin contaminar el núcleo

**Aceptada.** JC-IDE tiene tres capas: operaciones universales de editor, capacidades declarativas del perfil de lenguaje y extensiones especializadas opcionales. El núcleo no debe importar Progress, C# ni ninguna implementación de toolchain.

El primer desacoplamiento de código coloca los delimitadores de comentario y su preferencia en `jc-languages`; `jc-app` consume esa capacidad sin listas de lenguajes ni caso especial ABL.

Las herramientas ABL de formato/lexing/USING/OO/XREF/AVM se trasladarán conceptualmente al paquete interno `progress.abl`. Los `Progress*` del enum actual son deuda identificada: cuando exista un registro de contribuciones namespaced se migrarán sin romper los shortcuts de la TUI.

Las extensiones internas de lenguaje M3/M4 **no equivalen** al sistema de plugins externos/Sublime post-M5. No crear host Python, cargador dinámico ni sandbox completo antes de la fase acordada.

La búsqueda textual, edición de columnas, undo y snippets engine pertenecen al nivel universal; cada lenguaje aporta patrones, plantillas y semántica. Ver `docs/EXTENSION_ARCHITECTURE.md`. D13 mantiene CI remoto diferido hasta M5.

## D17 — Dos grupos de editor, navegación y Git transversal

**Aceptada en M2 como arquitectura; implementación parcial.** JC-IDE tendrá dos grupos de edición visibles simultáneamente en TUI/GUI, con estados de cursor/scroll/foco separados por grupo y `TabId`/`Document`/undo únicos. Una referencia anclada no se sustituye por navegación automática. La vinculación de scroll es independiente del anclaje. Historial de saltos atrás/adelante por grupo, inicialmente acotado a 128 ubicaciones.

Los `NavigationTarget`, Outline, Go to Definition, Open to Side y autocomplete son **universales**; cada proveedor de lenguaje resuelve símbolos. Progress ABL aporta includes, RUN, PROPATH y referencias XREF cuando hay evidencia. Las llamadas dinámicas permanecen no resueltas cuando no se pueda determinar el destino estáticamente.

Git es un proveedor transversal bajo demanda, no una función exclusiva de ABL. Los diffs de archivos/revisiones y snapshots inmutables se preparan en M2. Stage/unstage, commit, fetch/pull/push, branches, merge y resolución de conflictos en tres vías se implementan en M5 con confirmaciones y garantías de integridad. No ejecutar operaciones Git destructivas como efecto de navegar.

No se duplican procesos ni historiales para una segunda vista. Mantener la frontera de la decisión D16 y el aplazamiento de CI de D13.

Ver `docs/M2_NAVIGATION_PANES_GIT.md` y el prompt reproducible `docs/CONTINUAR_EN_EQUIPO.md`.

## D18 — Fundamentos antes de expansión; ecode como referencia adicional

**Aceptada como dirección de diseño el 2026-11-01; implementación F1–F3 pendiente.** La auditoría de la base `d5eccfc` reproduce fallos de undo multiposición, caret CRLF, estado de reapertura y costes de movimiento/posición proporcionales al archivo. Las pruebas funcionales previas no equivalen a aceptación arquitectónica.

Conservar bytes canónicos, encoding explícito, sesiones/guardado, comandos y fronteras de lenguaje. Sustituir propiedad duplicada de cursor/selección/viewport por vistas con identidad propia; introducir transacciones y mapas de cambio completos; después acceso incremental, presupuestos de memoria y render visible. Cada migración retira la ruta anterior, sin mantener otro motor de edición.

Esta decisión precisa D17: el documento tiene `DocumentId`; cada presentación tiene `ViewId`, dentro de un grupo. Compartir el `TabId` actual es una implementación transitoria, no el contrato final. Undo de contenido sigue único por documento y selección/viewport únicos por vista.

ecode/eepp aporta evidencia de flujos y estructuras, no una obligación de copiar su stack ni normalización textual. Sublime Text continúa como referencia visual. Medir localmente desde M2 y prototipar GUI tras estabilizar F1/F2, antes de completar toda la asistencia ABL. No elegir árbol/rope, GPU ni perfil release por reputación: comparar cargas, latencia y memoria.

M2 prepara y valida el contrato de tareas con un proveedor de prueba; la ejecución Progress real pertenece a M3. Se mantienen D13 (CI en M5), D14 (plugins Sublime post-M5) y D16 (proveedores opcionales). Corregir defectos conocidos de datos/edición ahora no depende de habilitar CI.

Evidencia y alcance: [AUDITORIA_ECODE_ARQUITECTURA.md](AUDITORIA_ECODE_ARQUITECTURA.md). Contratos: [ARQUITECTURA.md](ARQUITECTURA.md). Orden y criterios: [PLAN.md](PLAN.md).


### D18 — Avance F1 del 2026-11-01

Implementados DocumentId/ViewId, estado persistente único por vista, EditBatch/ChangeSet y propuestas textuales por revisión sin buffer mutable público. Los grupos mantienen sus vistas; el catálogo de pestañas TUI sigue visible globalmente. Se conserva una sesión/undo por documento; los cerrados retienen descriptores, no buffers. Undo/redo reubican posiciones de forma determinista; la restauración exacta de selecciones anteriores queda como mejora de UX con metadatos acotados. F2/F3 siguen pendientes. Evidencia y límites en [F1_DOCUMENT_VIEWS_TRANSACTIONS.md](F1_DOCUMENT_VIEWS_TRANSACTIONS.md).


## D19 — Índice de piezas por bloques y proyección textual acotada

Implementada en F2 sobre `ddc9d41`. Conservar el vector de piezas y añadir un índice de prefijos en bloques de 128, elegido tras comparar consultas y pesos simulados con vector plano y árbol Box. No se incorpora el árbol experimental al motor productivo ni se extrapola su prueba a transacciones completas.

Navegación, columnas y viewport consumen una proyección compartida por documento/revisión/encoding. EOL y checkpoints se actualizan en el intervalo afectado; las cachés de layout tienen cuota de 1 MiB/documento. El historial aplica 1024 pasos y 8 MiB de carga lógica por defecto; recolección de fuentes alcanzables preserva revisiones y undos retenidos. El snapshot de lectura actual es prestado; ownership/cuotas de jobs asíncronos pertenecen a F3.

Las cuotas no acreditan <50 MiB de RAM privada. Medir picos fríos, índices de líneas, capacidad de fuentes, muchos documentos y tecla-a-pantalla antes de aceptar objetivos globales. F3 sigue pendiente; sin cambios en D13 (CI remoto en M5), D14 (plugins post-M5) ni la referencia visual de Sublime Text. Datos y método en [F2_RANGES_INDEX_MEMORY.md](F2_RANGES_INDEX_MEMORY.md).


## D20 — Presentación prestada y proveedores internos con cuotas

Implementada localmente en F3 sobre `e72856f` (2026-11-02). Preparar viewport/texto antes de componer una `Presentation` prestada de solo lectura; el encoder terminal conserva solo frames entregados con éxito y emite diferencias. El loop despierta por entrada/completion, drena lotes acotados y bloquea en reposo.

Para workers internos elegir snapshots propios de rangos con cuotas, evitando prolongar préstamos de DocumentRead o clonar Document/FileSession/undo. JobId monotónico representa generación; validar documento/revisión/encoding/vista y clave vigente en cada entrega. Resultados tipados y propuestas sin aplicación automática; una aceptación futura usa la transacción F1. Dos workers lazy, ocho trabajos, 2 MiB por snapshot/8 MiB total, 64 elementos/64 KiB por resultado y 2 s por deadline predeterminados. Cancelación y timeout son cooperativos: proveedores confiables deben comprobar token y acotar sus operaciones; no se promete preempción ni aislamiento de plugins.

No incorporar proveedores ficticios a comandos visibles ni iniciar servicios al abrir archivos. Search Plus de proyecto necesita generación de contexto y escaneo por lotes; Progress real sigue M3. Los datos locales de inyección a VT no certifican pantalla física, RAM global ni cierre de M2A. Método y límites en [F3_PRESENTATION_TASKS.md](F3_PRESENTATION_TASKS.md). No cambia D13 (CI en M5), D14 (plugins post-M5) ni Sublime Text como dirección visual.


## D21 — Search Plus de proyecto bajo demanda y reemplazo por archivo

Implementado localmente sobre `ed51143` (2026-11-02). Un worker de proyecto reutilizado, última petición pendiente, snapshots propios de abiertos incluidos y resultados por lotes con revisión/generación/raíz. Compartir Wakeup/Cancellation con F3; el proveedor original ligado a documentos no se estira con documentos ficticios para representar el proyecto. KMP literal codificado estrictamente, tabla por encoding/consulta, globs acotados, coordenadas de bytes/escalares y cuotas explícitas. Sin indexador ni dependencias nuevas.

Preview es lectura del snapshot de búsqueda, sin I/O; aceptación explícita y comprobación por archivo al aplicar. Abiertos se editan con revisión esperada en un undo, sin guardar automáticamente; cerrados se guardan mediante FileSession. Procesar uno por turno permite cancelar los restantes, sin prometer rollback global ni preempción durante I/O. La UI informa guardado/edición/no-op/conflicto. Consultas incompletas, con avisos o fuentes obsoletas no habilitan replace.

Guardado conserva ReplaceFileW: retries solo para 32/33/1175, máximo cinco llamadas y 75 ms de esperas, comprobando huella entre intentos. 1176/1177 retienen temporal y ruta de recuperación. No se atribuye el handle histórico a un servicio ni se cambia el sistema. Detalles y regresiones en SAVE_WINDOWS_CONTENTION; uso, cuotas y límites en S1_PROJECT_SEARCH. Case folding Unicode completo/regex/S0 y metas globales siguen pendientes; no cambia la dirección visual Sublime ni los hitos de Progress/CI/plugins.


## D22 — S0 cooperativo y un solo matcher literal Unicode

Implementado sobre S1 `6e07b1e`, 2026-11-02. El documento se escanea por turnos con rangos prestados, evitando copias completas por tecla. 16 KiB/2 ms, 16 MiB, 10.000 resultados y 5 s por defecto; cancelación, revisión/encoding/generación y rechazo de reemplazos incompletos. Proyecto usa el mismo matcher, retirando el KMP anterior; conserva su opción exacta sensible a mayúsculas. Source maps se guardan solo para el prefijo coincidente.

Datos Unicode 17 fijados por hash/licencia: default full C+F sin Turkic, sin NFC/NFD ni conversión canónica. Las expansiones solo entregan rangos de escalares originales completos. Palabra es L/M/N/Pc + join controls, no UAX #29. Datos importados y algoritmos nuevos se identifican por separado. La dependencia es estática/generada, sin crate ni proceso de lenguaje nuevo.

Se implementaron navegación de selección, historial, marcadores y Go to escalar en comandos compartidos. Cancelar persiste al editar; Find/Replace explícitos reinician. No habilitar regex sin motor/presupuesto. La subida inicial de ~35 % del p95 de proyecto se recuperó en el corpus medido mediante decoder prestado/scratch reutilizado y rechazo ASCII dentro del mismo matcher; no se generaliza a todas las cargas ni se afirma mejora de cancelación. El perfil release se conserva. Detalle: S0_DECODER_OPTIMIZATION.md. RAM global/pantalla física permanecen abiertos. Desarrollo por bloques amplios y validación al cierre, según preferencia del usuario. Método en S0_DOCUMENT_SEARCH. El bloque M2_ACCEPTANCE_EVIDENCE amplía corpus, teclado y mediciones por proceso; retira la materialización completa del layout frío. El caso denso de ocho documentos todavía excede 50 MiB y la latencia física sigue sin medir; no aceptar objetivos globales por un corpus parcial.


## D23 — Metadatos compactos y recolección con identidad

La carga medida de ocho documentos de líneas cortas justifica sustituir inicio
usize/EOL u8 por un índice único compacto, con fallback de ancho sin truncar.
La recolección conserva arenas enteras todavía alcanzables, sin una copia por
cambiar presupuesto; mantiene mapas/revisiones/undo y compacta huecos reales.
No se cambia contenedor del core ni cuotas por intuición. Pico nativo 71,9 →
51,7 MiB y frío algo más lento: se acepta el ahorro medido, no la meta global de
50 MiB. Método/tradeoffs en M2_DENSE_LINE_METADATA.

## D24 — Disponibilidad en la entrada central de comandos TUI

Un item deshabilitado no puede ejecutar su handler mediante Enter de menú.
El dispatcher comprueba el registro actual antes de efectos de foco/búsqueda/
documento, para todas las entradas TUI. No se copian reglas por menú/widget.
La auditoría de teclado y sus seis regresiones están en M2_KEYBOARD_MATRIX_AUDIT;
no sustituyen prueba física de terminales ni hacen público el dispatcher para GUI.


## D25 — Objetos Git explícitos, inmutables y con proceso contenido

HEAD se fija a commit/OID; index a blob/modo stage 0 por ruta literal. El proveedor
lee objetos raw sin filtro/textconv, no worktree normalizado ni indexer permanente.
Git instalado se elige por ruta absoluta; no descarga ni hereda inyecciones Git del
entorno. Salidas y solicitud están acotadas; cleanup posee job/proceso/readers desde
el lanzamiento y no abandona un reader para aparentar deadline duro. La API Windows
se prueba localmente, sin fallback sin contención ni promesa de sandbox hostil.
Esto prepara el diff read-only, no lo entrega a UI: faltan jobs/revisiones/generación,
status y medición de auxiliares. Proveedor y límites en GIT_READ_ONLY_PROVIDER.
