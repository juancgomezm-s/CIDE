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
