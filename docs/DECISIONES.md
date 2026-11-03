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

## D26 — Entrega Git sobre el JobSystem existente

Implementada después de D25: un snapshot de trabajo acotado y un blob inmutable,
con OwnedDiffModel sobre el mismo algoritmo; no otro buffer editable ni pool.
Solo una comparación Git en vuelo y una intención pendiente sin fuentes; reservar
12 MiB de artefacto hasta consumir completion evita que resultados sin poll evadan
el presupuesto. Request descarta antes el resultado aceptado. Aplicación valida
revisión, encoding, vista activa, ruta y generación antes de presentar o navegar.
La TUI expone acciones centrales explícitas HEAD/index/refresh/cancel/configuración;
Git es opcional y se elige por ruta absoluta para la sesión. El visor prepara solo
filas visibles/prefijos acotados, con bytes inválidos/BOM/EOL explícitos. No ejecutar
Git ni diff durante entrada/render. Identidad de snapshot y refresh manual, sin
watcher ni status global. Evidencia y límites en GIT_DIFF_DELIVERY y
GIT_DIFF_PRESENTATION; memoria conjunta de auxiliares y pantalla física pendientes.

## D27 — Crecimiento de metadatos guiado por retención medida

El benchmark de Workbench real detectó que splice duplicaba capacidad del índice
para una sola newline sobre una tabla densa. Se conserva el índice único y sus
anchos; replace_after reserva primero la longitud final más holgura proporcional
acotada a 32 KiB lógicos, cobrando juntos los arrays Wide. No se cambia el core,
scanner frío, shrink, mapas o promoción lossless. El ahorro es capacidad retenida
de proyección, no RAM nativa. Un burst puede realocar más veces y el sufijo continúa
O(L). Se registra el caso frío de 8 MiB más lento sin inferir estadística de una
muestra. Método, antes/después y siguientes mediciones en M2_SUFFIX_COST_REVIEW.

La instrumentación opt-in de Git observa editor y descendientes por identidad
PID/creación y handles propios. Sus sumas son por ciclo de lecturas secuenciales,
con cobertura incompleta de procesos/picos breves. No convierte una captura pequeña
en aceptación global; GIT_AUXILIARY_MEASUREMENT conserva datos y limitaciones.


## D28 — Un índice de líneas por bloques después de comparación medida

La comparación histórica `191bd82` seleccionó bloques para integración, con
tradeoffs explícitos de construcción, lookup y líneas espaciadas. La migración
reemplaza D23/D27 en representación de líneas: no quedan arrays Compact/Wide ni
fallback plano productivo. Bases usize y registros relativos/EOL u16 conservan
offsets completos; máximo 256 registros/bloque y 14 bits relativos. Catálogo con
holgura proporcional limitada a 32 KiB de headers y liberación tras borrados
masivos; presupuestos de layout/historial/worker y propiedad documento/vista no
cambian. D23 conserva la reutilización de arenas alcanzables del core.

Se acepta la mejora densa medida, manteniendo sus costes: O(número de bloques)
para sufijos/catálogo, algo más de lookup caliente y metadatos mayores en líneas
muy espaciadas. Pruebas multibloque, legacy, bytes inválidos, CRLF, vistas y undo
pasaron. Pico denso editado 52,46→43,73 MiB; repetición 52,31→43,82 MiB. Undo
interno mejora en dos órdenes; el aumento de p95 VT inicial no persistió al
repetir. No se descarta esa primera cola ni se resta percentiles entre protocolos.
Método/datos en M2_BLOCK_LINE_INDEX y M2_UNDO_PIPELINE_PROBE. No se acepta RAM
global/pantalla física ni se cambia release/backend por intuición. Sin CI remoto.


## D29 — Estado Git con identidad de repositorio y entrega acotada

Status no pertenece a una revisión Document. Proveedor/parser y controlador
reutilizan transporte contenido/discovery y la frontera repository de ProjectSearch,
sin documentos ficticios. Un worker lazy reutilizable, activo1/pendienteúltima1/
completion1, generación y opciones completas, cancelación y Drop/join. Los paths
son bytes opacos, no autoridad filesystem; la observación no promete CAS global.

Consulta v2 NUL, no-renames/ignore-submodules, configuración global/system aislada.
Clean/process configurados se rechazan antes de status, conservando el contrato de
no ejecutar filtros y evitando falsear semántica suprimiendo conversiones. Config
estable y repositorio confiable son precondición, no sandbox hostil. Límites de
input/records/entries/path/owned y plazo único transporte+parseo. Validación y uso:
GIT_STATUS_READ_ONLY y GIT_STATUS_DELIVERY. Presentación TUI pendiente, Git writes M5.


## D30 — CPU por archivos con admisión y costes explícitos

El perfil identifica CPU de proyecto concentrada en un worker. Reemplazar la ruta
monolítica por un único executor con coordinador/scanner0 y helpers persistentes,
dos en total por defecto y 1…4 configurable. Headroom por available_parallelism,
sin afinidades/prioridades ni garantía de CPU libre externa. Reutilizar Pattern y
Request, sin clones de buffers abiertos; UI publica por ordinal y cobra cuotas
globales, con canales/batches acotados y cancelación/cleanup antes de reutilizar.
JobSystem conserva su identidad documental; no anidar pools ni inventar documentos.

La comparación local reduce espera y memoria muestreada a costa de más CPU total;
se registra ese intercambio, la mediana negativa pequeña y Qwen desconocido.
No speedup universal ni aceptación física/global. Overrides s/2/3 no justifican
cambiar la política global: mantener s. TUI expone --search-workers para limitar
demanda desde arranque; las escrituras/undo siguen en el owner, nunca en helpers.
Método/datos/pruebas/limitaciones: CPU_PERFORMANCE y CPU_PROJECT_EXECUTOR.

## D31 — Presentación de status sin convertir rutas en autoridad

Conectar el controlador existente a una vista de repositorio independiente de
DocumentId/encoding. Una sola aceptación en poll_tasks, contexto validado antes
de preparar y render de solo lectura. La vista posee selección/scroll, no otro
snapshot persistente ni sesión. Solo formatea filas visibles acotadas; paths
opacos se escapan sin pérdida y Enter no abre archivos. Navegación futura exigirá
validación propia del filesystem. Refresh/cancel explícitos, sin watcher.

La configuración de Git conserva la intención status/diff y Escape la descarta.
Un pánico obsoleto puede haber escrito a terminal: su señal de recuperación no
se descarta junto con la entrega. Se conserva aislamiento/filtros/límites del
proveedor, sin prometer igualdad con Git personal ni snapshot atómico del disco.
[Contrato y validación](GIT_STATUS_PRESENTATION.md): 290 pruebas completas locales,
Clippy/release; sin aceptación GUI/RAM global/latencia física ni escrituras Git.

## D32 — Rectángulos reales con admisión de geometría explícita

Implementar selección rectangular por vista, clipboard interno por filas y edición
atómica sobre la ruta F1 existente antes de exponer comandos. La TUI todavía no
tiene layout general de grafemas/ancho doble: admitir un repertorio cerrado de
una celda y TAB4/virtual, rechazar lo demás antes de editar y declarar el límite.
No un switch de modo columna completo ni una whitelist ampliada por intuición.

Caret/anchor de bytes siguen únicos, columnas/scroll visuales son metadata y
se invalidan por revisión/encoding. No exponer el rectángulo como selección lineal.
Una edición efectiva termina esa selección; clipboard tipado conserva filas y
ancho con tabs normalizados solo al copiar. No crear líneas tras EOF ni escribir
disco. [Contrato/validación](M2_RECTANGULAR_SELECTION.md): 310 distintas en cierre
selectivo, fmt/Clippy/release; GUI/Unicode general/pantalla física/RAM global abiertos.

## D33 — Join por cortes pequeños y Split por el comando existente

Preparar Join desde metadatos de líneas y rangos EOL/prefijo, sin reconstruir todo
el tramo seleccionado ni añadir mutación directa al core. Un solo batch por
revisión conserva undo/mapas; presupuesto previo1024 fronteras/2MiB del tramo,
sin atribuirle el coste total de layout/normalización F2. Preservar final EOL y
BOM, eliminar solo indentación ASCII de líneas siguientes y separar palabras
cuando procede. Selección de una sola línea y EOF sintético son no-op.

Split delega en insert_newline y Enter Editor resuelve el mismo CommandId; no hay
segunda regla de EOL/encoding. Ctrl+Shift+J evita quitar Ctrl+J al panel inferior.
Ambos rechazan rectángulos. [Uso y evidencia](M2_LINE_COMMANDS.md): nueve nuevas,
319 distintas acumuladas en cierre dirigido, fmt/Clippy/release; sin validación
física/RAM global ni escritura de archivos.

## D34 — Finder por metadatos y primer frontend Win32/GDI real

Implementar Quick Open del roadmap mediante controller lazy de metadatos, con
latest/generación/cuotas y apertura explícita. Reutilizar FileSession/documento
sucio y mover el lector confinado por handle de Search Plus a jc-files para no
mantener otro lector seguro. Default nueva apertura Quick Open16MiB; sin contenido
durante búsqueda ni indexador. [Contrato](M2_QUICK_OPEN.md).

Por autorización actual de Juan construir M2B ahora mediante Win32/GDI directo,
sin descargar framework. HWND/editor custom consume Workbench y presentación
visible; sin EDIT buffer/undo paralelo. No afirmar backend ganador por métricas
que no se midieron. Encoding inicial por CLI, clipboard interno, scalar-cell y
accesibilidad/IME/DPI/display pendientes; pruebas manuales al final. Mouse coloca
caret sin historial de salto y Save As deriva sesión. [Contrato/evidencia](M2B_NATIVE_WINDOWS.md).

339 pruebas distintas en cierre selectivo y16 checks nativos, formato/Clippy/
release. Continuar Quick Open/Palette GUI con notificación nativa, no perfiles
indefinidos ni regex/dependencias sin autorización.


## D35 — Selectores GUI sobre controller/registro y transporte opcional

Quick Open nativo usa el controller core; Palette filtra el registro y revalida
soporte/contexto antes de ejecutar. Menús/filas derivan shortcuts del KeyMap.
Consulta modal separada del documento; preparación visible no acepta completions.
Wakeup añade notifier opcional sin dependencia Win32 en jc-app; la GUI publica un
mensaje coalescido a su HWND, sin hilo puente ni polling. Mutex breve protege
detach/PostMessage; callback no posee punteros al modelo. Fallo de PostMessage
se registra con recuperación manual documentada. [Contrato](M2B_NATIVE_SELECTORS.md).
350 Rust/23 checks reales nativos pasaron; no inferir aceptación física/clipboard.


## D36 — Invisibles ASCII, coordenadas de display y campos multilínea compartidos

Mostrar invisibles escapa todo noASCII y controles con spans fuente, en vez de
atribuir un ancho a glifos invisibles/ausentes. CRLF es un span; caret/selección/
scroll convierten bytes explícitamente. Una cuota compartida lógica de layouts
se divide896/128KiB; no se hace otro buffer editable ni nueva afirmación de RAM.

Find/Replace de documento usan campos UTF8 acotados editables/navegables en ambos
frontends, EOL exactos y gestos F2/F4/F8 independientes de ejecutar. Se conserva
motor literal/S0 y una transacción en Replace All; no regex ni ámbito de proyecto
nuevo. Clipboard y encoding ya en curso se integran con handoffs validados, sin
llamadas OS manteniendo préstamos ni transcodificación implícita. Smoke no toca
el clipboard del usuario. [Contrato/evidencia](M2_INVISIBLES_MULTILINE.md):376 casos
Rust distintos por suites y29 checks nativos. Manual/global continúa abierto.


## D37 — Save All por documento y handoff de destino explícito

El frontend captura intenciones de guardado por DocumentId/revisión, hasta1024;
nuevos exigen Save As y cancel/error detienen restantes sin revertir los ya
guardados. SaveTarget revalida id/revisión/path/is_new tras modal sin préstamos.
Fuente/undo/vistas no se clonan; escrituras delegan a FileSession y mantienen
conflicto/no-clobber. Save As normal comparte el handoff. [Contrato](M2B_SAVE_ALL.md).

## D38 — CIDE visible con compatibilidad de ejecutables

Por decisión del propietario, CIDE sustituye la marca visible. Binarios canónicos
cide/cide-tui/cide-gui mantienen aliases jc-ide sin duplicar implementación. No se
renombran crates, raíz ni remoto, ni se reescribe evidencia histórica. Icono original
JG+CIDE verde para TUI/CLI; variante naranja GUI, procedencia/prompt conservados.
Recursos Windows se generan con std y MSVC ya disponible, sin dependencias nuevas.
[Inventario y validación](CIDE_REBRAND.md).

## D39 — GUI de proyecto consume S1 y abre por versión

Campos/resultados nativos acotados reutilizan el controller S1. Preparar presentación
solo invalida contexto; completions se aceptan por mensaje, no por pintura.
Apertura versionada compara antes de mutar catálogo/foco y recorre piezas prestadas.
Un resultado de disco conserva su validación de disco aunque ya haya abierto el
documento. No nuevo índice/motor ni edición directa desde worker/frontend.
[Contrato y evidencia](M2B_PROJECT_SEARCH_NATIVE.md).

## D40 — Preview de proyecto y Git readonly nativos sobre controllers existentes

La GUI mueve un único ProjectReplacePreview desde revisión a aplicación explícita,
un archivo por evento y cancelación de pendientes, sin undo global del filesystem.
La apertura cerrada reutiliza FileSession confinada/versionada, retirando doble
lectura y apertura genérica. No-op no toca historial ni disco. Git status conserva
identidad de repositorio; diff conserva snapshot/revisión/vista y F3, sin otro motor.
Separar validate_context de tick evita admitir/copiar al preparar una escena.
Solo mensajes aceptan completions; Git ejecutable se elige para la sesión y el
selector suelta préstamos/revalida contexto. El editor no escribe Git ni navega
paths status. [Contrato y evidencia](M2B_PROJECT_REPLACE_GIT.md).

## D41 — Catálogo Explorer único y enumeración Windows por handle

ProjectExplorer mantiene identidades nativas y un proveedor lazy acotado; ambos
frontends usan ExplorerView de solo lectura. Retirar la enumeración y aperturas
TUI anteriores evita dos modelos de árbol. Collapse expulsa descendientes; refresh
y raíz nueva invalidan IDs. Un cancelado rechaza incluso una completion ya emitida.
Windows fija ancestros ordinarios sin share-delete y enumera el handle final;
apertura revalida la cadena antes de reutilizar un documento dirty. No prometer
snapshot de FS ni confinamiento atómico del fallback no Windows. Labels escapados
solo presentan nombres; nunca son autoridad de paths. Pintura no acepta ni admite
tareas; GUI publica objetivos de mouse versionados solo en WM_PAINT.
[Implementación, cuotas y evidencia](M2B_SHARED_EXPLORER.md).

## D42 — Navegación nativa usa posiciones e historiales de aplicación

La GUI consume los cinco comandos de navegación existentes. Parser line[:column]
uno-based compartido con TUI y Workbench conserva validación escalar de fuente;
un modal solo posee input64B y destino document/rev/encoding/ViewId/pane. No preview
ni posición editable espejo. Back/forward y pin conservan semántica PaneManager.
Historia recupera query+flags de Workbench sin otro catálogo; palette mueve el
borrador único de Find/Replace para conservar reemplazo/campo al cancelar o recuperar.
Desarmar F3 diferido es independiente de parar el scan. Menús verifican disponibilidad
antes de cerrar overlays; pegado suelta préstamos y revalida destino/generación.
[Contrato/evidencia](M2B_NATIVE_NAVIGATION.md). Validación por bloque y sección
nativa dedicada evita repetir Explorer/perfiles cerrados.

## D43 — Edición GUI delegada y disponibilidad según el control de origen

Los seis comandos de líneas/comentarios consumen Workbench y perfiles existentes.
CommandContext.editor_input suma intención de fuente a foco Editor y selección
lineal; no se usa un campo modal como autoridad para editar el documento oculto.
Palette captura invocador y destino (document/rev/encoding/ViewId/pane/caret/anchor)
y revalida al aceptar, sin clonar editores. Menús comprueban disponibilidad antes
de cerrar overlays. Los bytes, transacciones, undo y geometría de vistas siguen en
aplicación; paint solo presenta. Semántica caretline/EOF y límites de asignaciones
existentes quedan explícitos en [contrato/evidencia](M2B_NATIVE_LINE_EDIT.md).

## D44 — Selección estricta y reapertura desde metadata, no recuperación

La selección literal usa S0/flags actuales y límites de rango y UTF8, rechazando
invalid decode/expansión antes de modificar query. GUI revalida control y origen
de paleta; cancelación/tabs usan el registro existente. try_reopen_closed propaga
lectura fallida y mantiene stacks; colisión exacta/canónica con un path abierto
falla explícitamente para conservar su buffer dirty. Solo metadata32, no documento
cerrado/undo; saved-file usa disco actual y new-file vacío. La confirmación dirty
revalida doc/rev/encoding/vista/pane/caret/anchor tras modal, delegando cuentas de
vistas a F1. [Contrato y evidencia](M2B_SELECTION_REOPEN.md).

## D45 — Rectángulo nativo comparte cuadrícula e intención visual

Entrada absoluta VisualPosition y relativa comparten admisión/LineLayout. Grid
TAB4 tiene prioridad mientras una vista posee rectángulo, incluso con invisibles;
marcas de celdas no alteran escala ni bytes. GUI/TUI retiran la ruta rectangular
de escapes y conservan preferencia diagnóstica para salir del modo. Paleta y drag
incluyen intención visual porque el bytecaret puede no cambiar dentro de TAB o
virtual. Captura/release fuera de préstamos, última selección válida al interrumpir,
stale sin reactivación; F1 posee transacciones, clipboard interno y vistas.
[Contrato/evidencia y límites](M2B_RECTANGLE_GUI.md).

## D46 — Paridad se inventaría por ruta, no por enum

El [inventario GUI](M2B_GUI_INVENTORY.md) registra los 104 comandos, bindings con
scope, menú/paleta, predicado, handler real y evidencia histórica limitada. Un
CommandOutcome::View sin consumidor gráfico no es funcionalidad. Menús Win32
adaptan Menu* sin simular OpenMenu TUI; un panel de proyecto no es un host genérico
Problems/Output. El cierre de edición/grupos solo conecta operaciones existentes:
Indent/Outdent/Join/Split comparten gate de intención real de fuente y enlace de
scroll usa el delta Workbench, independiente de pin/caret. No nuevos motores ni
proveedores. Los pendientes se concretan antes de ampliar la implementación.

## D47 — Paneles F3 comparten filas tipadas y origen capturado

TaskPanels reemplaza los strings TUI con IDs estables, texto único y origen por
stamp/ruta/rango; Output no gana permisos por tener metadata. JobSystem conserva
la ruta desde admisión para rechazar SaveAs. Un único poll GUI despacha Git antes
de otros resultados; presentación nunca acepta/admite. Navegación Problems solo
sobre documento abierto y contexto actual, preservando política de pin/grupos,
sin abrir disco ni aplicar propuestas. Estado vacío/error/stale/recorte es explícito;
el host no crea productores para poblarse. [Contrato/evidencia](M2B_BOTTOM_PANELS.md).

## D48 — Intención de campo y comando de paleta son destinos distintos

FieldSelection y edición/presentación compartidas gobiernan los tres selectores;
no otro buffer fuente ni undo. Entry_context habilita clipboard/SelectAll del
campo sin convertirlo en Editor; la lista de paleta conserva permisos del invocador.
Un handoff nativo captura generación/intención/candidato y contexto completo, suelta
préstamos, transfiere y revalida antes de mutar. Cuota256UTF8 se comprueba también
antes de reservar salida del decoder; no pegado truncado. [Contrato y evidencia](M2B_SELECTOR_INPUT.md).
La automatización usa solo backend sintético, sin leer/alterar clipboard privado.

## D49 — Gestos por vista y menús como intenciones revalidadas

Scroll manual pertenece a ViewState, sin desplazar caret ni crear historia. El
reveal se reanuda con intención explícita; scroll enlazado aplica delta consumido,
no sincroniza cursores ni depende de pin. Hits convierten la proyección vigente
a bytes legales. Captura solo conserva metadata de destino/ancla; cambio de
revisión/vista/encoding/ruta/raíz/foco invalida el gesto, con selección retenida.
SetCapture/ReleaseCapture y screen-coordinate/settings APIs quedan fuera del
préstamo. Menús se componen por modelo/categoría/handler, nunca rangos numéricos.
Una elección modal retiene un stamp acotado y revalida contexto/capability al volver;
macros de paleta abren el HMENU real sin perder draft invocador. Se eliminan las
rutas anteriores reemplazadas. [Contrato/evidencia](M2B_NATIVE_INTERACTION.md).

## D50 — Resaltado léxico compartido, incremental y acotado

La prioridad explícita de resaltado en ambas UI y Rust se implementa con lexer
de bytes en jc-languages, una SyntaxCache por documento en jc-app y presentaciones
de solo lectura. No se decodifica/copia el documento entero ni se duplican estados
editables. Las transacciones invalidan desde el primer cambio; encoding, ruta,
perfil y revisión identifican la caché. Contexto profundo continúa en eventos del
propietario, sin depender de pinturas. Cuotas:64KiB/tramo,128KiB sintaxis; layout
se reduce a768KiB y conserva invisibles128KiB dentro de1MiB/documento lógico.
Fallback Plain pendiente/limitado es explícito. ABL/Rust/JSON/JSONC son capacidades
léxicas parciales, sin parser ni semántica. [Contrato y evidencia](M2_SYNTAX_HIGHLIGHTING.md).

## D51 — Regex DFA compartido con trabajo contabilizado

La dependencia mantenida/fijada autorizada incorpora regex real en Scanner común
para documento y proyecto. Compilación con cuotas NFA/DFA/determinización;
ejecución por escalares con estado reanudable, chunks reutilizados y reintentos
cargados al presupuesto. Sin decode fuente completo ni edits desde worker.
Pattern compartido y reutilizado por revisión. Reemplazo conserva texto literal,
revisión esperada y una transacción. Limitaciones de case folding, fronteras,
complejidad adversaria y validación se explicitan en [M2_REGEX_SEARCH](M2_REGEX_SEARCH.md).

## D52 — Comparación de abiertos como artefacto dual F3

OpenDiffController valida ambos orígenes y generation; una ejecución en vuelo
con pendiente solo de metadata. F3 reserva el conjunto antes de copiar dos rangos
acotados y comparte Arc con OwnedDiffModel. La presentación no admite snapshots
ni acepta resultados, retiene solo filas/prefijos visibles y usa encoding por
lado. TUI bloquea edits del visor y navega al grupo de origen. No nuevo buffer,
undo, Git o ruta de escritura. [Evidencia y límites](M2_OPEN_DOCUMENT_DIFF.md).

## D53 — Capacidades internas opcionales, resultados tipados y activación explícita

SourceRegistry resuelve capacidades por perfil/ID namespaced, sin reglas ABL en
jc-app. Feature opcional de jc-progress aporta inferencia léxica; F3 conserva una
captura en vuelo y metadata pendiente. SourceItem separa navegar, referencia
relativa/no resuelta y reemplazo. Host valida modo, coordenadas, identidad y
revisión/caret antes de activar. Rutas ambiguas exigen elección y apertura por
handle; destino conserva/sugiere su propio encoding. Completado es texto local
anterior al caret y una transacción, no promesa semántica. [Evidencia](M2_SOURCE_SERVICES.md).
