# Recuperación de documentos y sesiones en Rust

Los metadatos gráficos v4 eliminan la ruta del ejecutable Git: el motor es embebido.
El lector acepta v1/v2/v3 y consume/ignora la antigua ruta de v3. v2 añadió
visibilidad del panel Código; al leer v1 se adopta Código visible por defecto.
Las opciones explícitas de arranque tienen prioridad.
La recuperación de documentos, generaciones, HEAD/BACKUP y fuentes no cambia.
La compatibilidad es de lectura hacia atrás; los ejecutables GUI antiguos
no interpretan los metadatos gráficos de versiones posteriores a la suya.

Bloque integrado y validado localmente, 2026-11-03. La evidencia final se registra
en `VALIDACION.md` y `evidence/gui-shell-browser-2026-11-03-*`.

El documento sigue teniendo un único propietario en el core. La copia durable
conserva arenas de bytes, piezas, revisiones, estado guardado, mapas de edición y
deshacer/rehacer. Las vistas guardan por separado sus posiciones, selecciones,
desplazamientos y grupos; al restaurar se reconstruyen sus identidades y sus
referencias a los documentos. No se clona un documento editable para cada vista.

`jc-files::recovery::RecoveryStore` escribe generaciones nuevas en archivos
exclusivos. Agrupa la escritura en bloques de64KiB, comprueba errores de escritura
y de vaciado, sincroniza los archivos y relee la generación para verificar tamaños
y fingerprints de contenido. Publica el manifiesto y después sustituye `HEAD`
atómicamente. `BACKUP` conserva la generación confirmada anterior; una escritura
incompleta no se publica. Los fingerprints detectan corrupción accidental, no son
firmas criptográficas ni protección contra un atacante que pueda reescribir el
depósito completo.

La carga valida primero la generación y después la estructura del documento y de
la sesión. Puede recuperar la generación anterior si la última está dañada. La
restauración se construye fuera del modelo activo y sólo se adopta completa.
Rutas Windows se almacenan como UTF-16 nativo, sin conversión con pérdida. Se
conservan bytes arbitrarios, codificación, BOM, finales de línea y la referencia
del contenido original en disco para detectar modificaciones externas al guardar.
Recuperar nunca escribe el archivo original.

Cada instancia posee un bloqueo exclusivo de su sesión. GUI y TUI usan depósitos
separados dentro de `%LOCALAPPDATA%\CIDE\recuperacion`. `CIDE_STATE_DIR` permite
redirigirlos y `CIDE_NO_SESSION=1` desactiva la recuperación. Las pruebas usan
únicamente carpetas propias bajo `work`; los modos smoke GUI no abren el depósito
personal y el anfitrión ConPTY desactiva la sesión salvo elección explícita.
La implementación de bloqueo durable del depósito está admitida en Windows;
en otras plataformas el lanzamiento conserva la sesión en memoria sin fingir un
bloqueo exclusivo de recuperación.

La recuperación periódica está integrada; ver [retención y programación](PERIODIC_RECOVERY_RETENTION.md).
También confirma al salir normalmente y restaura al iniciar,
incluidos documentos nuevos sin guardar. Si no puede confirmar la copia, no
cierra la ventana. Cerrar una pestaña con descarte explícito mantiene su semántica
propia. Un crash recupera la última generación confirmada; las teclas posteriores
a esa confirmación pueden no haberse conservado. Las pruebas de terminación abrupta
del proceso no equivalen a una prueba de corte eléctrico o fallo del hardware.

La salida general de la GUI conserva la ventana cuando existen cambios
pendientes y la recuperación está desactivada para esa ejecución. Muestra
el motivo en el estado. La ruta de salida ya no ofrece descartar todos
los documentos como alternativa a la confirmación de la sesión.

La admisión actual limita cada documento a256MiB de fuentes y cada sesión a512MiB
codificados, además de cuotas explícitas de vistas, piezas e historial. Rechaza el
checkpoint antes de publicarlo si después no podría restaurarlo con esos límites.
El depósito conserva HEAD/BACKUP y limpia versiones obsoletas de forma segura, con
cuota de8GiB por sesión; si los bloqueos impiden liberar espacio, falla de forma
visible y conserva los datos.
Estas cuotas pertenecen al motor actual en memoria. La integración de documentos
de varios GB sigue siendo un bloque
pendiente; el prototipo de lectura paginada no demuestra su recuperación completa.

## Evidencia de cierre

Pasaron siete pruebas del codec/preflight core, cinco del almacén, una de manifiesto
hostil, cinco del puente FileSession, tres de vistas, cuatro de Workbench y cuatro
de recuperación de frontends. La prueba de proceso termina el hijo con código86
durante una escritura y justo después del commit: conserva respectivamente la
generación anterior y la nueva. El helper ignorado se invoca desde su prueba padre.

El recorrido ConPTY `evidence/gui-shell-browser-2026-11-03-tui-recovery.json` ejecuta
tres procesos reales, confirma cambios pendientes antes/después de dos reaperturas,
deshacer/rehacer restaurados y archivo original intacto. GUI también pasó tres
procesos reales, ocultos mediante STARTUPINFO, con mensajes enviados exclusivamente
al HWND del PID propio. Un inspector Rust limitado a fixtures verifica bytes,
dirty, historial, caret y reutilización del documento recuperado al abrir su ruta
por CLI. Informe: `evidence/gui-shell-browser-2026-11-03-gui-recovery.json`.
El primer intento del harness ConPTY llegó a recuperar
el texto pero agotó su plazo de entrada Ctrl+Z; el recorrido final invoca los menús
españoles para comprobar deshacer/rehacer de forma determinista. Se conserva el
informe inicial; no demuestra corrupción de fuente ni valida ese transporte Ctrl+Z.
