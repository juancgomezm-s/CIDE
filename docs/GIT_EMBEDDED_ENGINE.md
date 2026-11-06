# Motor Git embebido de CIDE

Bloque de adaptación a biblioteca Rust, 5 de noviembre de 2026. Sustituye la
selección y detección de `git.exe` del bloque anterior. Esta nota describe la
implementación de lectura y su cierre local. No declara terminadas las escrituras o red.

## Motor y distribución

La evolución se rige por [núcleo Git compartido](GIT_SHARED_CORE.md): una biblioteca
independiente reutilizable por CIDE, OmniCore y OmniCoder. La integración de otros
hosts y los bindings aún no están implementados ni validados.

`jc-git` es el proveedor independiente de interfaz y del núcleo de bytes.
Utiliza `gix = 0.90.0`, fijado con `default-features = false` y las capacidades
`status`, `revision`, `sha1` y `sha256`. El workspace fija Rust 1.88.0.
Los ejecutables GUI y TUI compilan el motor como biblioteca; no hay un programa
Git que el usuario deba localizar ni un runner de consola como fallback.

Las licencias originales de gitoxide se conservan en
`third_party/gitoxide/LICENSE-MIT` y `LICENSE-APACHE`. El inventario
`third_party/gitoxide/DEPENDENCIES.json` identifica las dependencias nuevas,
versiones, autores, licencias, checksums de paquetes y avisos copiados; sus textos
se conservan en `third_party/gitoxide/dependencies/`. Ese directorio debe viajar
con la distribución. No se sustituyen las atribuciones por la marca de CIDE.

Git de consola puede aparecer en el código de pruebas para preparar repositorios
y actuar como oráculo. Es una herramienta del entorno de desarrollo, no una
dependencia de ejecución de los controladores GUI/TUI ni del motor `jc-git`.

## Lectura realmente conectada

El adaptador `jc-app::git_read::GitReader` ya no recibe ruta de ejecutable.
Delegan en `jc-git` la apertura de raíz, resolución de HEAD, árboles, índice y
blobs exactos. Los objetos se inspeccionan antes de cargar su contenido; el blob
entregado se vuelve a verificar contra su identificador. Se preservan BOM,
bytes legacy o inválidos, EOL y ausencia de salto final; no se decodifica el
objeto para almacenarlo ni se sustituye el buffer editable por una copia Git.

El estado compara el árbol de HEAD con entradas stage 0 del índice, clasifica
conflictos mediante sus stages y consulta índice frente al directorio de trabajo
con la API de gix. Añade no rastreados y omite ignorados según la política actual.
Los resultados se ordenan mediante colecciones ordenadas y se mapean directamente
a los tipos de estado de `jc-app`; no pasan por texto porcelain de un proceso.
Los parsers porcelain retenidos sirven a pruebas/modelos existentes y no son
la ruta productiva del motor nuevo.

La GUI y la TUI conservan sus controladores y sus presentaciones acotadas.
Estado Git pertenece al repositorio; el diff del documento activo mantiene su
snapshot, revisión, codificación y vista. La aceptación de entregas sigue en
`poll_tasks`/la ruta de tareas: un resultado obsoleto no puede volver a mostrarse
por pintar, mover el cursor o pulsar una fila vieja. Pintura y composición no
abren repositorios ni ejecutan consultas.

## Solo lectura y opciones locales

La apertura exige raíz absoluta y verifica que el workdir real coincida con ella.
Reconoce gitfile/commondir para la disposición de worktrees. Las opciones aisladas
de gix evitan heredar configuración global y entorno; se deshabilitan includes,
reemplazos de objetos, fsmonitor, caché de no rastreados, hooks y mantenimiento.
Se rechazan definiciones de filtros `clean`, `smudge` o `process` antes de usar
la consulta de estado. No se ejecutan esos auxiliares para aparentar compatibilidad.

El motor no guarda un refresco del índice, no modifica configuración, refs u
objetos, y no obtiene objetos desde la red. Estado rechaza repositorios con
gitlinks en índice o HEAD: no informa «limpio» sin comprobar los submódulos.
No se habilita detección de renombrados/copias. Los símbolos
de hash admitidos por la compilación no equivalen a aceptación de todos los
formatos de repositorio: el código rechaza configuraciones de partial clone o
ref storage alternativo e índices sparse. Split-index y otros formatos necesitan
su propia evidencia antes de prometer soporte.

## Cotas y cancelación

| Recurso | Cota actual del proveedor |
|---|---|
| Archivo de índice o de configuración preflight | 8 MiB cada uno |
| Gitfile y commondir leídos como control | 32 KiB cada uno |
| Índice cargado | 65 536 entradas; 8 MiB de respaldo de rutas |
| Recorrido del árbol HEAD | 65 536 entradas visitadas, 8 MiB de rutas acumuladas y profundidad controlada a 128 |
| Ruta de resultado | Hasta 4096 bytes, con límite menor configurable |
| Resultados | Hasta 16 384 entradas y 4 MiB de presupuesto lógico, con límites menores del adaptador |
| Blob | 2 MiB por defecto desde `jc-app`; hasta 16 MiB configurables |
| Contenido de archivo rastreado durante estado | 16 MiB antes de normalizar/hash |
| Plazo de operación | No mayor de 30 s; valores menores del controlador |

Estas cotas describen recursos admitidos, no el pico total de RAM del proceso.
Gix puede mantener estructuras internas y asignaciones transitorias; el tamaño
del índice se comprueba antes de parsear y otras estructuras se revisan después
de cargarse. Hace falta medir los picos y el comportamiento en repositorios
grandes antes de aceptar la puerta global de memoria.

El trabajo de Git se realiza en el worker existente. Cada operación activa
tiene un monitor Rust de cancelación que espera en intervalos de 5 ms; no es
un pool adicional de procesamiento. Gix recibe el flag y la visita de entradas
también comprueba las cuotas. Se pide un hilo de trabajo para la consulta de
estado. Un plazo vencido marca la interrupción y el resultado se rechaza.

La cancelación es cooperativa: una llamada de sistema, descompresión o lectura
que no observe el flag puede retrasar el retorno. No se mata un hilo Rust ni se
afirma una cota dura de tiempo, contención de memoria por proceso o aislamiento
frente a un repositorio hostil. El bloque anterior con Job Object de un proceso
Git fue retirado y no se presenta como garantía del motor en proceso.

## Sesiones e interfaz

La GUI publica metadatos de sesión versión 4 sin ruta Git. Lee las versiones
1–3; en v3 consume y valida la ruta antigua para mantener el formato y después
la ignora. Se conservan documentos, undo/redo, paneles, vistas y numeración.
Las opciones antiguas de línea de comandos admitidas por compatibilidad tampoco
convierten esa ruta en autoridad para lanzar un programa.

La TUI retiró campo de ejecutable, prompt de Git, destino pendiente de
configuración y autodetección de programa. Sus consultas solo necesitan el
contexto del repositorio. La identidad histórica `GitConfigureExecutable`, mientras
permanezca registrada por compatibilidad, explica que Git está integrado y no
abre un prompt ni crea un trabajador. La sesión no introduce una segunda copia
editable del documento para almacenar estado Git.

## Evidencia de cierre local

`work/embedded-git-integration-tests.log` registra 283 pruebas de aplicación y
sus grupos de integración, además de 75 pruebas GUI y seis de entrada aprobadas.
El fallo inicial TUI era un oracle de encabezado que esperaba «aislado»;
`work/embedded-git-final-tests.log` registra su cierre: 153 pruebas TUI de
biblioteca y cinco de entrada, más 11 pruebas del motor. Son 533 aprobaciones
por suites, no una corrida global del workspace. Dos helpers ignorados por
ejecución normal se invocan expresamente desde sus respectivos padres.

El mismo registro muestra aprobada la prueba TUI padre/hijo que prepara un
repositorio propio con el oráculo y luego ejecuta estado y diff en un hijo con
PATH vacío y sin configuración del oráculo. No hay prompt de ejecutable y el
padre compara índice, configuración, HEAD, refs y fuentes para detectar escrituras.
El helper tiene plazo externo y solo termina el proceso recién creado si excede
el límite; no toca instancias del usuario.

El motor contrasta estado con el oráculo en repositorios sintéticos; cubre
objetos sueltos y empaquetados, BOM/bytes legacy/EOL, conflictos, autocrlf,
HEAD sin commit, worktrees, cuotas, filtros externos y plazo vencido. Rechaza
índice/config de más de 8 MiB y blobs cuyo contenido no corresponde al OID.
Conserva dos registros si un borrado preparado comparte ruta con el archivo
no rastreado retenido en disco. Todas estas pruebas comparan las fuentes y
metadatos antes/después para detectar escrituras.

Formato, Clippy de workspace/todos los destinos y release offline de todo el
workspace aprobaron con Rust 1.88.0. Los seis ejecutables se encuentran en
`target/embedded-git-next/release`; no se reemplazaron los binarios anteriores.
El recorrido GUI nativo pasó 135 checks con PATH vacío, sin cambios en fuentes,
índice, config, objetos o refs. Informe:
`work/native-gui-smoke/20261105T221435Z-ebb68d461f42`.
No acredita escritorio físico, ConPTY ni nuevas pruebas de accesibilidad/IME.

## Coste observado y puertas pendientes

El lock añade 127 identidades de registro, 52 de la familia gix; incluye
dependencias condicionales, no todas enlazadas en Windows. No se habilitaron
features de red, credenciales o paralelismo global. La descompresión usa Rust.

| Ejecutable | Anterior | Embebido | Diferencia |
|---|---:|---:|---:|
| GUI | 3 705 344 bytes | 5 983 744 bytes | +2 278 400 bytes |
| TUI | 2 868 224 bytes | 5 131 776 bytes | +2 263 552 bytes |

La compilación release del workspace tardó 1 min 9 s en esta captura local,
con cachés parciales; no es una medida universal ni de compilación limpia.
Arranque GUI oculto, una muestra de calentamiento y siete medidas: mediana
49,89 ms, p95 53,44 ms, máximo 2,95 MiB privados hasta respuesta WM_NULL.
Recuperación desactivada y fuente sintética de 315 líneas. No incluye una
consulta Git grande ni demuestra fluidez física. Evidencia:
`work/m2-gui-startup-39b26fd8b0684335b636802a28e41af6/report.json`.
El intento pareado del ejecutable anterior no completó su cierre normal en
5 s; no se publica una mejora/regresión de arranque a partir de esa comparación
incompleta (`work/embedded-git-startup-before.log`). Solo se terminó el proceso
recién creado por el propio harness.

Quedan para su admisión: picos de memoria/tiempo sobre índices y packs grandes,
aislamiento mediante helper Rust propio si esos picos lo exigen, submódulos,
split-index y evidencia SHA-256 completa. Escrituras Git, merge/rebase,
red y credenciales siguen en M5. No se declara paridad avanzada con Git.

## Pendientes de alcance

El bloque implementa lectura local. Stage/unstage, commit, checkout/restauración,
merge/rebase, push/fetch y autenticación no se habilitan por haber enlazado gix.
Son construcciones posteriores con validación y recuperación de escrituras;
la política vigente las sitúa en M5. No se anuncia paridad completa con Git ni
se recupera un `git.exe` oculto para cubrir una capacidad ausente.
