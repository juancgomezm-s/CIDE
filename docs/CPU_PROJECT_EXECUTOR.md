# Search Plus — ejecución por archivos en varios núcleos

**Estado vigente:** integrado y validado; [comparativa, CPU total y límites](CPU_PERFORMANCE.md). Las notas de entrega del worker describen la construcción anterior a la ventana del integrador.

Estado de construcción: código completo en `project_search/scan.rs` y
`project_search/parallel.rs`; sin Cargo, fixtures ni medidas ejecutados por este
worker. La integración del controlador, tests externos y perfil comparativo
pertenece al integrador. No se declara mejora medida del código nuevo.

## Decisión y API

El integrador seleccionó la frontera por archivo tras perfilar consultas de
16 MiB/16 archivos con CPU agregada aproximadamente equivalente a un núcleo.
La [revisión de fuente](CPU_PARALLELISM_SOURCE_REVIEW.md) explica esa frontera.
La elección no acredita todavía speedup, menor CPU total o latencia de teclado.

`parallel::Executor::new()` crea metadata sin helpers. El worker existente de
ProjectSearch conserva un único Executor fuera de su loop y llama
`run(Arc<Request>, publish) -> ProjectSearchState` dentro de su catch_unwind.
Root integra `options.workers`, validado 1…4 y predeterminado 2. El número efectivo
es el menor de requested, cuatro y
`available_parallelism().saturating_sub(4).clamp(1,4)`. Se reservan así plazas
lógicas de forma conservadora; no se mide carga Qwen ni se infiere CPU libre de
los 16 hilos/8 núcleos declarados. No cambia afinidad, prioridad ni timers del SO.

El coordinador actual ejecuta también el scanner0. Con W workers efectivos hay
W−1 helpers, creados perezosamente y reutilizados. Si falla su creación se reduce
la concurrencia; la ejecución sigue siendo válida. Workers previamente creados
pueden permanecer ociosos al pedir menos concurrencia. El máximo creado son tres
helpers más el coordinador, sin un pool adicional F3 ni threads por archivo/query.
No se fabrican DocumentId/TaskStamp para rutas de disco ni se amplía JobSystem.

## Un solo engine y orden

Se retira el `scan::run` monolítico anterior. Walker conserva abiertos primero
en orden BTreeMap y luego DFS de read_dir, exclusiones/globs, límite de entradas,
profundidad 32, rechazo de links/reparse, errores tipados como filas y validación
de raíz/handle. El scan-file único usa el mismo Pattern/Scanner/decoder y lógica
de snippet; se invoca igual desde scanner0 y helpers. Workers=1 usa ventana uno
de ese engine, sin mantener otro algoritmo secuencial productivo.

El coordinador admite ondas de hasta W archivos. Ejecuta primero la ordinal
menor y asigna las siguientes a helpers. Cada archivo posterior tiene un
`sync_channel(1)` propio. Los resultados se drenan por ordinal y por offset, nunca
por quién acabó primero. Los notices de recorrido y las barreras de límite se
posponen tras los archivos anteriores. No se conserva un catálogo global para
ordenar al final: hay hasta W descriptores de archivo y uno diferido, dentro de
la cota 2W. El siguiente recorrido se admite tras consumir la onda actual.

El primer archivo se publica sin esperar al resto: flush al terminar scanner0
y cada archivo posterior, además de los batches llenos. Un archivo lento puede
retener la onda y dejar helpers ociosos; este head-of-line es un coste explícito
de mantener orden y memoria acotados. Orden determinista significa **dado el mismo
traversal**. read_dir no garantiza orden lexicográfico estable entre SO/ejecuciones;
no se promete esa propiedad nueva ni una instantánea atómica del proyecto.

Cada scanner procesa un archivo desde su origen: folding, posiciones escalares,
CRLF, BOM, páginas legacy y política no solapada no se cortan por límites entre
workers. Proyecto conserva búsqueda literal sensible a mayúsculas, validación de
UTF-8 y encoding explícito legacy; no se añaden switches ficticios. El patrón
compilado se comparte como Arc una vez por petición.

## Bytes y propiedad

Los abiertos permanecen en el Request propietario, prestados a scanner0 o mediante
Arc<Request> a helpers. No se copia su Vec de bytes de nuevo ni se clona Document,
FileSession, selección o undo. Las cuotas vigentes siguen siendo 8 MiB de abiertos
por petición y una activa más una pendiente (hasta 16 MiB de bytes capturados),
sin multiplicarlas por W.

Antes de despachar, el coordinador reserva bytes **de fuente** en orden. La suma
de examinados previos y reservas de la onda no supera max_scan_bytes. Para abiertos
se conoce longitud exacta; para disco se usa longitud de metadata y se pasa ese
cap al reader verificado. Cada reporte devuelve longitud realmente capturada;
lo reservado de más se libera al completar la onda. El contador global no es una
cuota por worker. Fuentes con NUL/encoding no admitido cuentan sus bytes leídos
con éxito igual que antes; errores de lectura no publican matches ni suman fuente
examinada. Pueden generar notices consumiendo la cuota global de resultados.

Un archivo que crece por encima de la longitud admitida produce Issue antes de
buscar o publicar matches. Esto es más estricto que antes: ya no se permite que
crezca desde su metadata inicial hasta el límite general de 2 MiB. Si encoge,
se busca y fingerprinta el contenido capturado. Se conserva revalidación de
fingerprint/revisión al navegar/reemplazar; ni stat ni concurrencia aportan CAS.

`read_file` ahora prueba crecimiento con **un byte de stack separado**. No llena
un Vec hasta L y luego lo duplica para leer L+1. La capacidad de fuente crece
acotada al cap de lectura; Interrupted se reintenta igual que read_to_end. El
reader compartido sigue admitiendo crecimiento dentro de su cap para sus otros
callers, pero el executor fija ese cap a la metadata reservada del archivo.

max_scan_bytes conserva su significado de bytes de fuente examinados, **no**
todos los bytes de I/O. El byte guardia, lecturas fallidas, metadata y posteriores
revalidaciones de disco no se confunden con ese contador. Por eso una suma exacta
del tamaño de fuentes puede terminar Complete; no se agrega un byte por archivo
a esa cuota. Como ajuste de frontera, un archivo de disco vacío se admite aun
cuando no quede payload, igual que un abierto vacío; antes la comprobación de
remaining==0 podía terminar Limited pese a ese archivo de cero bytes.

## Resultados y memoria acotada

El coordinador aplica max_results y los 2 MiB lógicos globales con el cargo
ProjectSearchRow vigente. Los helpers no compiten por reservar las primeras N
filas. Cuando aparece una fila adicional que no cabe, termina Limited, cierra
staging posterior y conserva únicamente el prefijo ordenado aceptado. No se
publica una captura truncada como Complete. Un archivo que por sí mismo agota
el límite del Scanner también produce Limited después de sus filas admitidas.

Batch tiene como máximo 32 filas y 16 KiB de cargo de staging, incluyendo capacidad
del Vec, PathBuf y strings, con cargo conservador no menor al anterior. Se limita
antes de reservar/insertar. Un único row que exceda ese cap produce Limited en
vez de una asignación ilimitada; con rutas admitidas de 4096 unidades y snippets
de 80 escalares cabe el caso habitual. Una raíz/error con path excesivo también
queda sujeto a este límite nuevo. El canal UI mantiene sus cuatro batches y un
terminal administrados por el controlador; no se aumenta esa cola.

Los costes lógicos nuevos que deben medirse aparte del resultado aceptado son:

- Hasta W buffers de disco, cada uno dentro del cap actual de 2 MiB; no un archivo
  adicional por descriptor diferido. El guard de crecimiento ocupa un byte por
  lector fuera del Vec de fuente.
- Por scanner activo, buffer de chunk de 16 KiB más lookahead y mapa de orígenes
  KMP por prefijo. Compartir Pattern no comparte ese mapa: un patrón largo puede
  consumir más que el chunk. Sigue limitado por la consulta máxima de 4096 bytes.
- Residual de hasta 256 SearchMatch por Step del Scanner existente. No se crean
  snippets para todos los matches de un archivo antes de aplicar backpressure.
- Por helper, hasta un batch en canal, uno local y uno que esté drenando el
  coordinador, más una fila scratch: cota conservadora de 64 KiB de staging por
  helper (64 KiB default; 192 KiB con W=4), aparte de buffers/mapas y filas globales.
  Solo un archivo está siendo drenado por el coordinador a la vez.
- Canales, locks, handles de threads y descriptores son metadata adicional acotada
  por W. Vec/Arc/capacidades lógicas no equivalen a RAM privada o working set.

No se promete que el presupuesto global del proceso permanezca idéntico: el
paralelismo tiene este coste explícito. No se atribuye al modelo una aceptación
global menor de 50 MiB ni se eleva el presupuesto de fuente por usar más núcleos.

## Cancelación, pánico y quiescencia

Cada onda tiene un Stop interno distinto al token/deadline de la petición.
Limited detiene los helpers con ese Stop, sin convertir el estado en Cancelled.
El token externo se comprueba entre archivos, turnos y resultados; el deadline
original no se reinicia en cada helper. I/O arbitrario puede bloquear y prolongar
el cierre: no se promete cancelación dura de filesystem cloud/remoto.

Wave es RAII: en salida normal, límite, cancelación o unwinding marca Stop,
**elimina receivers antes de esperar idle**, y despierta por cierre a cualquier
send bloqueado. No hace join sosteniendo un mutex ni abandona JoinHandles. Los
helpers capturan pánicos de scan-file y el coordinador los propaga al catch_unwind
padre, incluso si otra rama ya decidió Limited/Cancelled. La señal se comprueba
de nuevo después de que todos los helpers estén idle. No se hace panic desde Drop.
La recuperación de terminal sigue siendo del padre.

Execute consume Work por valor. Fuente/scanner/patrón y Arc<Request> desaparecen
antes de publicar busy=false y volver a la espera del slot. Done/Rows no retienen
Request; no queda last_request entre consultas. La próxima onda solo reutiliza
slots quiescentes. Al cerrar el executor se notifica shutdown y se hace join de
todos sus helpers. Los consumidores de UI siguen descartando generaciones/revisiones
obsoletas con los mecanismos del controlador.

## Regresiones construidas y validación pendiente

Tres tests internos usan solo buffers propios: cierre de receiver tras observar
producción real y liberación del último Arc antes de idle; reutilización de helper
con rangos/posiciones exactas y batches acotados; y pánico inyectado del coordinador
con productor activo, comprobando limpieza RAII y liberación del snapshot. No se
ejecutaron durante construcción ni crean repositorios/procesos externos.

El frente de tests del integrador construye casos API workers1/2/4: igualdad de
filas/orden/encoding/offsets, cuota global de resultados y bytes exactos, fuentes
abiertas sucias prioritarias y stale, supersession/cancel y Drop con cola sin ack.
El integrador ejecutará fmt, pruebas y controles al cerrar todo el bloque, y
comparará el mismo perfil inicial con sus muestras CPU/wall. Esos resultados,
concurrencia efectiva y límites observados deben registrarse antes de afirmar
mejora, aceptación nativa o cierre del objetivo de CPU.
