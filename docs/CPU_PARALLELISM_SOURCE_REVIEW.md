# CPU y paralelismo — revisión de fuente

**Revisión histórica de fuente `16071e3`:** las líneas de la tabla corresponden a ese checkpoint. Tras el perfil se implementó la opción 1; [executor actual](CPU_PROJECT_EXECUTOR.md) y [evidencia](CPU_PERFORMANCE.md). La opción 2 sigue diferida.

Diseño basado en la fuente actual, sin perfil, benchmark, Cargo, Git ni procesos
auxiliares ejecutados por este frente. Los puntos siguientes son candidatos de
coste y dependencias comprobadas en código; **no son cuellos medidos ni una
promesa de speedup**. Solo se propone elegir entre las dos opciones siguientes
después de la ventana de perfil del integrador.

## Fronteras y costes actuales

| Área | Evidencia en fuente | Consecuencia para CPU/paralelismo |
| --- | --- | --- |
| Proyecto | `project_search.rs:223` captura abiertos; `:284` crea un worker reutilizado; `:434` ejecuta `scan::run` completo | Hay una ejecución de proyecto a la vez. Capturar ya copia hasta 8 MiB de abiertos por generación; no es un escaneo paralelo |
| Recorrido/archivo | `project_search/scan.rs:129`, `:146`, `:162`, `:235`, `:321` | Primero abiertos del BTreeMap, luego DFS de disco. Lectura, validación, fingerprint, matcher y snippets de un archivo terminan antes de pasar al siguiente |
| Pasadas del proyecto | `scan.rs:332–364`; `jc-text/src/encoding.rs:44` | `contains(NUL)`, probe/validación UTF-8, fingerprint de disco y búsqueda pueden recorrer bytes varias veces. El coste relativo frente a I/O/metadata no está medido |
| Snippets densos | `scan.rs:379–392` | Cada hit llama `decode_fragment` sobre hasta 160 bytes y luego crea otro String limitado a 80 escalares. Ese decoder materializa también offsets/incidencias (`encoding.rs:106`), aunque aquí solo se usa texto |
| Patrón | `scan.rs:143`, `:370`; `text_search.rs:196` | El patrón ya se compila una vez por petición y se comparte como Arc entre scanners. No justificaría recompilarlo por worker |
| Documento S0 | `workbench/search.rs:84`, `:125`, `:134–157` | Programar es barato; el escaneo real ocurre en slots cooperativos del caller con lecturas por rangos, no en JobSystem. No hay una copia entera de documento para el scanner |
| Scanner/decoder | `text_search.rs:405–424`, `:473`; `encoding.rs:168–201` | Se reutiliza el Vec, pero se recrea DecodedScalars al reanudar cada slot. `from_utf8(remaining)` puede revalidar el sufijo ya validado de un chunk si el turno anterior salió por 256 hits o tiempo. Verificar su peso antes de paralelizar el mismo trabajo redundante |
| Estado Unicode | `text_search.rs:220–338`, `:450–463`; `unicode_search.rs:6`, `:25` | KMP, mapa de orígenes, match pendiente, palabra previa y CRLF son estado secuencial. El folding puede expandir un escalar; offset de byte y columna escalar no son intercambiables |
| Pool F3 | `jobs.rs:216–237`, `:327`, `:511`, `:627–688` | Ya hay pool perezoso, dos workers por defecto, configurable 1…8. El mutex protege pop/publicación; el proveedor corre fuera del lock. Un único trabajo no se reparte automáticamente entre esos workers |
| Admisión F3 | `jobs.rs:51–68`, `:279`, `:450–559`, `:576–622` | Captura desde DocumentId/TaskStamp, copia el rango a Arc una vez, limita 8 trabajos/8 MiB de snapshots/12 MiB de artefactos por defecto y retiene reservas hasta poll. No acepta actualmente una tarea de archivo de disco/proyecto independiente |
| Diff | `diff_model.rs:182–195`, `:341`, `:376`, `:409–448`, `:452` | OwnedDiffModel comparte dos Arc sin copiar bytes. Indexa ambos lados en serie; cada capa Myers depende de la anterior y el traceback de ese trace. Presupuesto y desempate son comunes a toda la ejecución |

Las operaciones de filesystem de `scan::read_file` (`scan.rs:50–79`) conservan
validación de raíz/handle/reparse y lectura acotada. Paralelizarlas no permite
saltarse esas comprobaciones ni convertir una llamada bloqueante al filesystem
en cancelación preemptible. El fingerprint de contenido y TaskStamp siguen
siendo necesarios aunque el reparto de CPU cambie.

## Opción 1 — Archivos de proyecto, contrato concreto preferido tras perfil

**Unidad:** archivo completo para la semántica Unicode. Reutilizar el worker
actual de ProjectSearch como coordinador y scanner de la primera ordinal de una
ventana; añadir helpers persistentes dentro del **mismo executor de proyecto**.
Default **dos workers en total**, contando coordinador/scanner0; configurable
1…4, nunca cuatro más el coordinador. Con uno no hay helper activo: mismo engine
y ventana uno, sin conservar dos algoritmos serial/paralelo diferentes.

La API actual `JobSystem::submit` no sirve directamente para disco: exige un
DocumentId vivo, construye su TaskStamp y vuelve a copiar su rango. Fabricar un
documento o usar la misma JobKey para todos los archivos rompería identidad o
cancelaría hermanos (`jobs.rs:531`). Migrar proyecto a un executor F3 genérico
requeriría ampliar contextos, admisión y salida tipada; no es un requisito ni
una capacidad ya existente de este bloque. No anidar este trabajo en un proveedor
F3 que espere a sus hijos; no crear otro pool desde cada archivo o petición.

El controlador sería dueño de coordinador/helpers, creados perezosamente y
reutilizados entre consultas. Un cambio de consulta cancela la generación vieja
y conserva una sola pendiente; no crea otra familia de threads. Root conserva
ProjectSearchOptions/controlador. Si se elige implementar, scan.rs y parallel.rs
compartirían un único scan-file con sink acotado; firmas aquí son propuestas.

**Concurrencia:** solicitar dos por defecto, limitar por configuración (máximo
cuatro), `available_parallelism` y el headroom que admita el caller. El equipo
declarado tiene ocho núcleos físicos y 16 lógicos; eso no prueba CPU libre. La
carga Qwen es desconocida. `available_parallelism` informa disponibilidad para
el proceso, no utilización actual del equipo. No lanzar 16 workers por detectar
16 lógicos, ni cambiar prioridad/temporizadores del SO. El pool F3 y otros
servicios siguen contando aparte al fijar el headroom global.

### Ventana, canales y semántica

- `FileWork { generation, ordinal, path, source, reserved_bytes, request }`;
  `source` distingue abierto con TaskStamp de disco. Un Pattern Arc por petición;
  un Scanner propio por archivo conserva estado KMP/Unicode/CRLF de inicio a EOF.
- Como máximo `2*W` descriptores pendientes y `W` fuentes activas. El coordinador
  descubre una ventana en orden, procesa su primera ordinal y asigna las otras
  a los helpers. Drena y publica resultados por ordinal antes de admitir la
  siguiente ventana. Así el scanner0 no se bloquea esperando a otro worker para
  producir/publicar su propia primera ordinal. El coste de ondas desiguales y
  head-of-line queda explícito: si un archivo domina, habrá capacidad ociosa.
- Cada helper devuelve por un canal `sync_channel(1)` exclusivo de su archivo.
  Un batch tiene **hasta 32 filas o 16 KiB de carga**, lo que ocurra primero;
  también hay un batch local y como máximo una fila scratch. No hay colección
  ilimitada por archivo. El sender bloqueado sale al cerrarse su receiver.
- El coordinador publica con el canal UI existente (cuatro batches y un terminal)
  y cobra centralmente las primeras N filas y sus bytes aceptados. Los resultados
  provisionales de archivos posteriores no consumen por carrera el max_results
  global. Al llegar a un límite cancela ese trabajo restante y termina Limited,
  nunca Complete con una lista truncada ni un max_results por worker.
- El Scanner actual permite hasta 256 hits por slot (`text_search.rs:473`). Hay
  que contabilizar ese residual acotado o añadir límite de hits al mismo engine
  para avanzar según crédito del sink; no suponer que ya produce 32 ni duplicar
  el matcher. La salida parcial de un worker no cambia la política no solapada.

**Orden:** abiertos primero, en el orden actual de su BTreeMap, y luego disco sin
duplicar esos paths. Los helpers no eligen qué archivos ganan la cuota. Ordinal y
offset determinan entrega y límites para el mismo recorrido. `read_dir` ya no
promete orden lexicográfico portable: no se promete reproducibilidad entre SO o
mutaciones externas sin añadir un catálogo ordenado con su presupuesto. Nunca
acumular 100.000 rutas/resultados solo para ordenar al final.

### Reservas previas y propiedad

El coordinador conserva `examined + held_reservations <= max_scan_bytes`, admite
en orden y devuelve reserva no consumida al recibir la terminación ordenada.
No asignar el presupuesto completo a cada worker. Un descriptor de disco lleva
longitud de metadata comprobada; el cap de lectura respeta esa reserva y
max_file_bytes. Si crece, Issue y descarte; si se encoge, cobrar lectura real y
devolver lo no consumido. El `read_file` actual lee hasta `limit+1` para detectar
crecimiento: reservar también ese centinela antes de despachar si se quiere una
cota estricta de bytes leídos. No afirmar que metadata.len reserva todo ese I/O.
Con reserva conservadora puede aparecer Limited en una frontera de presupuesto
que no deja un byte de guardia; esa política debe quedar en pruebas/documentación,
no convertirse silenciosamente en Complete ni exceder el máximo.

Cada archivo de disco conserva fingerprint de los bytes realmente capturados;
stat no lo sustituye ni garantiza una instantánea atómica ante cambios concurrentes.
Se mantienen validación de raíz/handle/reparse, detección de crecimiento y
revalidación al seleccionar/reemplazar. Deadline/cancelación no interrumpen una
llamada arbitraria a filesystem o un volumen cloud/remoto bloqueado.

Los abiertos ya están copiados en Request: hasta 8 MiB por generación. Envolver
el Request propietario en Arc y prestar sus OpenSource evita **otra copia de esos
8 MiB por helper**; clonar solo cabeceras Arc y metadata necesaria. Una activa y
una pendiente conservan el máximo actual de hasta 16 MiB de bytes abiertos,
no un presupuesto por worker. No introducir clones de Document/FileSession/undo.

El coste nuevo debe cargarse explícitamente: hasta W archivos de disco activos
de 2 MiB, buffers Scanner de 16 KiB más lookahead por activo, y staging por helper.
El staging debe contar capacidad de Vec, paths y snippets, no solo filas ocupadas.
Canal, batch local y scratch de hasta 16 KiB permiten reservar conservadoramente
64 KiB por helper: **64 KiB con W=2, 192 KiB con W=4**, aparte del máximo global
actual de 2 MiB de filas aceptadas. No confundir esa cota lógica con RAM. Los
buffers de lectura se cargan por capacidad real, no solo longitud; read_to_end
puede reservar de más. No sumar esos costes como memoria gratuita por compartir
Arc. Estas reservas son condición de admisión, no crecimiento tácito de cuotas.

También hay estado por matcher: `origins: VecDeque<Origin>` (`text_search.rs:235`)
crece con el prefijo activo y la longitud plegada del patrón. Compartir Pattern
no comparte ese mapa de orígenes. Debe presupuestarse por capacidad real de cada
scanner, junto al residual Step de hasta 256 matches si no se reduce su cap.
Un patrón largo/prefijo repetitivo puede pesar más que el buffer de 16 KiB;
el cálculo anterior de staging no pretende incluir todo ese estado.

`Arc<Request>` solo vive en la generación activa y WorkItems que la usan. Cada
helper toma un item fuera del lock, mueve su salida sin ese Arc y **suelta item,
fuente/scanner y clones locales antes de volver a wait**. Un batch/Done no retiene
Request entero. No dejar `last_request`, closures o un scanner cacheado con Arc
de la generación vieja entre esperas. La cancelación libera descriptores pendientes;
la activa no libera sus reservas hasta confirmar salida de todos sus consumidores.
El owner cancela, cierra canales y hace join al retirar helpers/cerrar controlador;
no abandona JoinHandles ni reutiliza un slot aún ocupado por la generación vieja.

### Stop y fallos

Separar token/deadline de usuario de un stop interno de ejecución por límite.
`Limited` no debe obtenerse llamando al mismo `request.cancel.cancel()` que al
final se traduce en Cancelled. El coordinador es el único que fija el motivo
terminal; helpers observan stop para dejar de calcular/emitir. Al parar, cerrar
receivers y despertar condvars para soltar productores y consumidores; no hacer
join manteniendo el mutex ni esperando un sender cuyo receptor se dejó vivo.
El pánico conserva recuperación de terminal y falla la generación. Al entregar,
revalidar TaskStamp/encoding/generación de abiertos como hoy y conservar
fingerprints para navegación/reemplazo de disco.

Esta frontera no corta Unicode dentro del archivo y preserva sus mapas. Es la
opción de menor riesgo semántico si domina el trabajo de muchos archivos. No se
afirma que un archivo único ni todas las cargas se aceleren.

## Opción 2 — Rangos de un solo snapshot, condicionada y de mayor coste

Solo explorar si el perfil identifica un archivo grande como coste dominante y
la copia/admisión cabe en las cuotas. No sustituye la ruta por rangos prestados
de S0 para todos los documentos. JobSystem admite 2 MiB por snapshot y 8 MiB en
total por defecto; S0 puede recorrer hasta 16 MiB por defecto sin esa copia.
No se puede mover automáticamente esa carga al pool conservando el mismo coste
de memoria ni elevar las cuotas solo para conseguir más ocupación de CPU.

**Unidad propuesta:** rangos grandes (p. ej. 256 KiB como punto de perfil, no
umbral aceptado), dos activos, sobre **un Arc de bytes acotado por revisión**.
Habría que añadir admisión que comparta ese Arc y cargue sus bytes una vez; la
captura actual de `submit` copiaría cada rango/halo separadamente.

El Scanner actual no tiene constructor de rango con estado inicial. Iniciar
dos scanners desde cero y deduplicar por inicio es incorrecto: para `aa` en una
secuencia de `a`, la fase local puede producir rangos incompatibles con los
matches no solapados del recorrido completo. La emisión actual reinicia estado
al aceptar un match (`text_search.rs:259`). Folding y whole-word añaden fronteras
adicionales, no resuelven ese problema.

La ampliación necesaria es un modo **del mismo Matcher** que enumere candidatos
solapados válidos con mapa original, más un reducer ordenado que aplique la
política global no solapada y el máximo de resultados. No otro engine ASCII ni
un filtro de duplicados que descarte candidatos necesarios. Acotar candidatos
por paso y memoria agregada; si agotan presupuesto, resultado Limited explícito,
sin afirmar completitud. En corpus repetitivo ese tráfico puede superar mucho
el número final de matches.

Los extremos deben caer en fronteras reales de decoder, manteniendo agrupación
de inválidos, BOM solo en byte cero, escalar previo/vecino para whole-word y CRLF.
Para un patrón de M escalares plegados, el halo debe cubrir al menos el contexto
de M−1 escalares originales más vecinos de frontera; el byte fijo por sí solo
no determina las columnas ni el folding. Con UTF-8, una cota conservadora de
bytes de halo crece como 4×M más contexto. Un patrón largo puede hacer ineficiente
partir los chunks actuales de 16 KiB. No asumir que cuatro bytes de lookahead
del decoder también bastan como halo del matcher.

Las posiciones escalares requerirían checkpoints iniciales válidos para la
revisión, o resúmenes relativos por rango y un prefix-sum ordenado que ajuste
líneas/columnas y CRLF antes de publicar. Construir esos checkpoints decodificando
todo el documento en la UI anularía la frontera buscada. No usar offsets bytes
como posiciones Unicode. Un paso/reducer/continuación se agenda en el mismo pool;
ningún proveedor ejecuta compute y espera a subtareas dentro de sus dos workers.

**Coste:** copia inicial propia donde S0 no la necesita, dos buffers más halos,
resultados candidatos, checkpoints/resúmenes y reducer. Patrón y snapshot se
comparten por Arc; no copiar Document/FileSession/undo. Conserva el stamp completo,
generación y cancelación por rango; datos de otra revisión/encoding nunca se
reinterpretan ni se convierten en propuestas de edición actuales.

## Qué perfilar antes de elegir

El integrador puede separar lectura/canonicalización, probe/fingerprint,
Scanner/decoder/folding y snippets, más espera de cola/UI. Para S0 interesa
contar bytes validados por UTF-8 frente a bytes avanzados y número de reanudaciones
del mismo chunk, especialmente con matches densos y expiración de slot. Así se
distingue trabajo serial evitable de falta de núcleos. Ese contador no existe
todavía; `scanned_bytes` mide avance, no suma de bytes revalidados por from_utf8.

Comparar una y dos plazas del mismo executor con idéntico corpus, cuotas,
warmups, orden de resultados, cancelación y versiones. Corpus relevantes: muchos
archivos pequeños/medianos frente a uno grande; ASCII/Unicode/legacy explícito;
consulta ausente frente a densa; fronteras de UTF-8 inválido/CRLF/BOM; edición y
cancelación mientras llega backlog. Registrar bytes copiados, capacidad retenida
y CPU/wall por fase; un tiempo menor de fin no prueba menos CPU total ni mejor
latencia de teclado. Las pruebas deben comparar filas/rangos/orden contra la
ruta secuencial y conservar presupuesto global/retorno de cancelación bajo carga.

No priorizo paralelizar Myers en este bloque: las capas y desempates actuales
son dependientes, y el Budget combina trabajo, líneas y metadatos. Dar una cuota
completa por hilo cambiaría el límite conjunto; atomizarla sin política podría
hacer depender el punto de Limited del scheduling. Separar solo el indexado de
los dos lados comparte bytes por Arc, pero necesita coordinación sin child-wait
y reservas conjuntas; no hay evidencia de que esa fase domine el diff. Los diffs
independientes ya pueden ser trabajos distintos cuando su controlador lo permita.

**Prioridad propuesta:** perfilar primero los costes concretos indicados y elegir
la opción 1 si domina CPU por muchos archivos. Mantener la opción 2 como trabajo
condicionado por fuente única y presupuesto; no elegirla por número de núcleos
sin demostrar ventaja frente a copiar/coordinar y repetir validación.
