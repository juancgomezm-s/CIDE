# M2-F — Índice de líneas por bloques integrado

El índice plano fue sustituido por un único índice por bloques en
`jc-app/src/workbench/line_index.rs`, después de la
[comparación experimental](LINE_INDEX_CANDIDATES.md) conservada en `191bd82`.
No hay selección entre dos modelos productivos ni otro documento/historial.
Los ejemplos comparativos se retiraron al migrar; se reproducen en ese checkpoint
aislado. Los ejemplos Workbench suffix_cost y cold_growth siguen operativos.

## Representación y límites

Cada bloque contiene como máximo 256 registros `u16`: 14 bits de desplazamiento
relativo y dos de EOL. Una base `usize` conserva la coordenada absoluta completa;
si el siguiente inicio no cabe en el rango relativo, comienza otro bloque. No
se trunca un offset de más de 1 GiB ni `usize::MAX`; no hace falta promover todo
el índice a dos vectores anchos. Se retiró la operación privada de promoción
que había quedado vacía. Bytes, escalares y columnas continúan siendo fronteras
distintas: TextProjection conserva la validación textual de CRLF/BOM/encoding.

El catálogo guarda base y primer ordinal. Obtener una línea busca su bloque;
upper_bound busca catálogo y registros. Un sufijo completo desplaza bases por
bloque; su frontera parcial se reconstruye localmente. Replace conserva el EOL
de la cola, reempaqueta fronteras/vecinos/reemplazo y ajusta ordinales posteriores.
Catálogo y ordinales aún tienen coste O(número de bloques); no se declara O(log L)
para todas las ediciones ni se reconstruyen todos los registros por cada tecla.

El catálogo crece con holgura proporcional limitada a 32 KiB lógicos de headers.
Tras un borrado masivo reduce catálogo solo si sobran más de 32 KiB y la capacidad
supera el doble de bloques vivos. Evita conservar headers de cientos de miles de
líneas cuando quedan una o dos, sin shrink en cada edición. Los payloads crecen
dentro de bloques pequeños. Contabilidad de proyección conserva capacidad heap
de catálogo/payload y cache, sin añadir otra vez la raíz inline; no es RAM privada.
Cache de layout 1 MiB/documento y presupuestos de historial/snapshots no cambian.

La representación favorece líneas cortas. Líneas separadas por más de 16 KiB
pueden requerir un bloque por registro, con más headers/asignaciones que el plano.
La construcción y acceso por ordinal también pueden ser más lentos; esos costes
se midieron antes de elegir y se mantienen en la documentación, no se ocultan
como una optimización universal.

## Validación funcional del bloque

La suite completa aprobó 245 pruebas tras la sustitución, incluyendo nuevas
transacciones reales multibloque UTF-8/BOM/bytes inválidos/EOL mixtos y
Windows-1252 explícito, undo/redo desde vistas distintas y escaneo acotado de
4 MiB densos. Una revisión posterior añadió liberación de catálogo masivo y su
regresión: se repitieron las 15 pruebas de índice/proyección y tres de aplicación
afectadas. Son **246 pruebas distintas aprobadas al cierre**, sin repetir los
crates no afectados por ese ajuste de capacidad. Dos helpers ignorados se invocan
desde sus tests padres. Formato, Clippy workspace/all-targets con warnings denegados
y release de bins/ejemplos aprobados.

Los oráculos incluyen miles de entradas, borrados/reemplazos que cruzan bloques,
inversas acumuladas, 512 roundtrips dispersos y offsets cercanos a usize::MAX.
El nuevo caso reduce 400001 entradas a una/dos, comprueba retención acotada y
restaura miles de fronteras con geometría/EOL correctos. No se clona un Document,
FileSession o historial para dividir vistas; los snapshots Git permanecen aparte.

## Modelos reales antes y después

Dos pares de procesos con orden invertido, binarios planos `d6de969` guardados
antes de compilar la migración y binarios por bloques identificados por hash.
Mismo código/protocolo para cada ejemplo, sin compilaciones/fixtures concurrentes.
Cold es índice/layout de la instancia, no purga de cachés de SO/CPU/allocator.
Cada par conserva 930 filas por versión de cold_growth y 4914 de suffix_cost.

8 MiB/inicio, p95 en µs (primer / segundo orden):

| Flujo de Workbench | Plano | Bloques |
|---|---:|---:|
| Primera edición de cold_growth | 2373,1 / 2989,2 | 60,1 / 54,2 |
| Newline caliente, apply | 572,1 / 560,6 | 32,4 / 27,9 |
| Newline caliente, undo | 1144,4 / 1147,1 | 25,0 / 23,4 |
| Movimiento + consulta caliente | 2,9 / 2,7 | 3,4 / 3,3 |

El movimiento aumenta ligeramente y la edición densa reduce el coste del sufijo
en estos casos. Construcción de índice de cold_growth: p95 de 8 MiB/inicio,
43,475/42,459 ms planos frente a 43,508/42,412 ms por bloques; no es el tiempo de
apertura completa ni prueba de disco frío. El benchmark puro de construcción y
get sigue mostrando su tradeoff, según LINE_INDEX_CANDIDATES.

Datos por prefijo `M2-block-index-{cold_growth,suffix_cost}-{flat,block}-r{1,2}`:
[primera distribución plana](benchmarks/M2-block-index-cold_growth-flat-r1-samples.csv),
[por bloques](benchmarks/M2-block-index-cold_growth-block-r1-samples.csv),
[sufijo plano](benchmarks/M2-block-index-suffix_cost-flat-r1-samples.csv),
[por bloques](benchmarks/M2-block-index-suffix_cost-block-r1-samples.csv).
Cada prefijo contiene raw/percentiles/metadata y hash del ejemplo/binario/índice.

## Aplicación nativa y alcance de aceptación

El mismo RunAcceptance midió ocho procesos dense-edits-block (uno descartado y
siete retenidos), más 24 procesos base/multi/long (uno descartado por forma).
Son 32 recorridos completos / 28 procesos retenidos, sin otra carga pesada
concurrente. Cierres limpios, hashes de disco, edición/undo y navegación pasaron.

| Corpus | Máximo histórico de compromiso del editor |
|---|---:|
| Ocho documentos densos de 4 MiB, edición/undo | 43,727 MiB |
| Ocho documentos de 4 MiB, líneas de 64 B | 34,906 MiB |
| Una línea de 16 MiB | 17,957 MiB |
| Documento base | 1,414 MiB |

En el corpus denso: antes de editar, medianas por proceso 43,051–43,141 MiB;
tras editar y undo, 43,449–43,727 MiB. Frente al plano acotado, el máximo histórico
baja de 52,461 a 43,727 MiB. **Este corpus local queda bajo 50 MiB**; no equivale
a garantía global con todos los servicios/cargas/máquinas ni cierra pantalla
física/GUI. Bootstrap, conhost y harness siguen en columnas separadas.

ConPTY actual denso: movimiento p95 16,1337 ms, inserción 16,2674 ms y undo
20,9661 ms. El modelo mejora, pero ese tail de undo no se oculta ni se atribuye
sin prueba al núcleo. La [comparación de pipeline y repetición nativa](M2_UNDO_PIPELINE_PROBE.md)
ya cerró esa investigación local: undo interno total p95 plano 573,7/752,9 µs
frente a bloques 179,2/172,1 µs en dos órdenes. En la repetición VT, p95 plano
16,9702 ms y bloques 16,9147 ms; la diferencia inicial no persistió. No se borran
las colas originales ni se acepta la meta global de latencia.
ConPTY mide inyección-a-VT, no la pantalla física.

La repetición añadió 16 procesos / 14 retenidos: total del bloque 48 / 42,
además de las cuatro capturas in-process. Pico histórico denso repetido por
bloques 43,820 MiB (plano 52,313 MiB); sigue siendo aceptación de ese corpus local.

[Memoria densa cruda](benchmarks/M2-evidence-dense-edits-block-native-memory.csv),
[checks](benchmarks/M2-evidence-dense-edits-block-native-checks.csv),
[tiempos](benchmarks/M2-evidence-dense-edits-block-native-timings.csv),
[metadata](benchmarks/M2-evidence-dense-edits-block-native-metadata.json).
[Otras formas](benchmarks/M2-evidence-block-native-memory.csv) y
[tiempos](benchmarks/M2-evidence-block-native-timings.csv).
