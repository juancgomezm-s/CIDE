# Comparación experimental de índices de líneas

> Checkpoint experimental histórico `191bd82`. Los ejemplos y la implementación
> plana descritos aquí se reproducen en una copia aislada de ese commit. Después
> se integró una única ruta productiva por bloques y se retiraron ambos ejemplos:
> [diseño, validación y mediciones actuales](M2_BLOCK_LINE_INDEX.md). Los comandos
> de este documento corresponden al checkpoint histórico, no al árbol actual.

Driver y candidato completos, revisados, compilados y medidos por el integrador.
La estructura sigue fuera del editor en este checkpoint. Los resultados permiten
seleccionarla para una integración posterior, con tradeoffs y revalidación nativa.

## Frontera experimental

El [contrato](LINE_INDEX_CANDIDATE_CONTRACT.md) separa implementación y comparación.
`crates/jc-app/examples/line_index_candidates.rs` incluye directamente el archivo
productivo mediante `#[path = "../src/workbench/line_index.rs"]`, sin copiarlo ni
publicar sus tipos privados. Incluye también `support/block_line_index.rs`.
Adaptadores locales convierten ambas entradas a `(usize, u8)` exclusivamente
para comparación. No añade dependencias, manifests, APIs públicas, documentos
editables, decodificación de fuentes ni otro índice productivo.

La referencia plana empaqueta offset absoluto y EOL en `u32` cuando es posible,
y promueve a offsets `usize` y EOL separados cuando hace falta. El candidato
usa bloques con base `usize` y offsets relativos/EOL compactos, conforme a su
archivo de soporte. El driver no selecciona ni favorece una estructura por
una predicción de consumo. Los contratos F1/F2/F3 continúan siendo productivos:
bytes originales, un documento/historial, coordenadas explícitas, lecturas por
rango, revisión/encoding y presupuestos no cambian en esta comparación.

## Cargas idénticas y oráculo

Se construyen metadatos aritméticos de 1, 4 y 8 MiB con stride 8 y 64 bytes.
Se añaden dos casos patológicos de 8 MiB con stride 8192 y 65536: los saltos
relativos grandes pueden reducir fuertemente la ocupación de cada bloque y
hacer dominante su catálogo. Las ocho cargas son iguales en ambas estructuras.
Cada línea completa termina en LF y se añade el inicio vacío de EOF: el número
de registros es `source_bytes / stride + 1`. El driver no crea archivos ni
almacena textos decodificados. Un `Vec<(usize,u8)>` independiente representa
las entradas esperadas y el límite de bytes. El índice plano no es el oráculo
del candidato: ambos se comprueban por separado contra ese vector.

Las regresiones pequeñas cubren índice vacío, única línea, BOM representado por
inicio 3, una línea extrema de 16 MiB sin frontera adicional, mezcla de los tres
metadatos EOL, línea final sin EOL, offsets que exceden el empaquetado de `u32`,
`usize::MAX`, promoción por reemplazo y desplazamientos reversibles próximos a
`usize::MAX`. Son casos de metadatos; no acreditan escaneo/decodificación de un
BOM o CRLF desde archivos. Los valores de EOL se conservan sin reinterpretar.

Una secuencia determinista de 96 inserciones escalares y de frontera acumula
cambios antes de deshacerlos en orden inverso. Después de cada paso se comparan
todas las entradas/EOL, longitud, `first_start`, `get`, `start`, `ending` y
`upper_bound` frente al vector. La búsqueda del oráculo usa `partition_point`
de la biblioteca estándar sobre las entradas originales, sin llamar a búsquedas
de ninguna estructura comparada. Se comprueban inicios, byte anterior, interior
de intervalo, cero, final y `usize::MAX`.

Además, un oráculo acumulativo comienza con **8193 entradas**, offset inicial 3
y EOL 1/2/3 alternados. Ejecuta 96 pasos en cuatro familias: inserta 600
fronteras, reemplaza 600 por 700, borra 512 y reemplaza 900 por 650. Las posiciones
se distribuyen de forma determinista alrededor de fronteras de 256 registros;
todo tramo eliminado no vacío atraviesa varios bloques aunque cambie su
fragmentación. Las anchuras nuevas varían de 4 a 16 bytes y sus EOL varían.
La secuencia mantiene al menos 6705 entradas. Después se aplican las 96
inversas en orden de stack, conservando antes las anchuras/EOL originales.

Para estos tramos el índice recibe `shift_tail(last+1, old_end, new_end)` y
`replace_after(first+1..last+1, replacement)`. El oráculo reconstruye el vector
esperado directamente desde prefijo, segmentos nuevos, cola con EOL conservado
y sufijo desplazado; no invoca ese protocolo. Se comparan todas las entradas
en cada paso y una distribución de hasta aproximadamente 4096 fronteras para
get/búsqueda. La reversión completa debe recuperar todas las entradas y el
límite original. Esto cubre reemplazos de múltiples bloques y no solo el
caso de una inserción de línea y su inversa inmediata.

En los corpus grandes se comparan todas las entradas en el primer y último par
de edición/inversa de cada familia; las comprobaciones adicionales de acceso y
búsqueda se limitan a aproximadamente 4096 fronteras distribuidas. Las consultas
del benchmark también se verifican contra el oráculo antes de cronometrarlas.
Todos estos checks y la construcción del oráculo quedan fuera del timer.

## Protocolo de tiempos

Cada estructura recorre las mismas ocho cargas. Cada familia medida utiliza cinco
calentamientos descartados y 31 muestras conservadas. No se vacía la caché del
SO ni se inicia un proceso nuevo por muestra. `cold_build` significa construir
un índice nuevo desde metadatos ya disponibles; no significa disco, CPU o
allocator del proceso realmente fríos. La construcción usa `push` para los
registros previos y `finish` para el último registro sin EOL, como el scanner
productivo; la destrucción del índice queda fuera del tiempo de construcción.

`get_hot` y `upper_bound_hot` usan exactamente el mismo lote determinista de
4096 consultas para cada estructura, generado antes del timer. Incluyen el
bucle de consulta y `black_box`; no se resta un coste de bucle estimado.
`elapsed_ns` es el tiempo del lote completo. Dividir por 4096 proporciona el
coste medio por consulta de ese lote; el p95 de lotes dividido por 4096 **no es**
un p95 de latencia individual de consulta.

Las familias de mutación son `scalar` (inserta un byte ASCII sin nueva frontera)
y `newline` (inserta `x\n`), cada una en `begin`, `middle` y `eof`. La posición
media es el inicio de línea `entries.len()/2`; EOF es el registro vacío final.
Por cada muestra se prepara un índice nuevo y se llama `shrink_to_fit` fuera del
timer. Los reemplazos propios de ambos sentidos también se construyen antes
de medir. Cada `*_forward` incluye la primera reserva necesaria desde ese
índice compacto; cada `*_inverse` opera sobre el resultado de esa inserción y
puede conservar su capacidad. Son ediciones de primera reserva, no ediciones
calientes con holgura heredada de los calentamientos descartados.

La inserción de frontera aplica el protocolo común del contrato:

```text
shift_tail(i+1, N, N+2)
replace_after(i+1..i+1, [(start_i, LF), (start_i+2, unknown)])
```

La inversa desplaza desde `i+2` de `N+2` a `N` y elimina el inicio `i+1`,
conservando el EOL de la cola. El oráculo modifica su vector de entradas
directamente; no usa `replace_after`, snapshots de índices ni sus helpers.
Para un byte escalar solo se desplazan inicios posteriores al de la línea
contenedora. En EOF esa operación puede no modificar metadatos: se conserva
como caso explícito, sin extrapolar su coste a edición real de bytes o layout.

## Memoria lógica y salidas

El programa emite CSV por stdout con columnas:

```text
implementation,source_bytes,stride,operation,sample,batch_size,elapsed_ns,lines,capacity_slots,logical_bytes
```

Los tiempos usan nanosegundos; las capacidades y bytes lógicos se obtienen
fuera del timer. `batch_size` vale 4096 para consultas, 1 para construcción o
mutación y 0 para filas descriptivas de retención. `sample` es 0…30 en series
medidas; no es número de documento ni cantidad de consultas.

`logical_bytes` cuenta el índice raíz más capacidades de sus asignaciones.
La API plana devuelve solo arrays, por lo que el adaptador añade
`size_of::<LineIndex>()`; el candidato ya incluye raíz, headers/catálogo y
arrays en `memory_bytes()`, sin añadirlos dos veces. `capacity_slots` conserva
la capacidad reportada por cada API y no se interpreta como bytes. Las filas
`retained_built` y `retained_shrunk` distinguen capacidad retenida tras
construcción natural y tras compactación explícita. Cada forward/inversa
registra también su capacidad final, incluida la reserva que conserve el undo
de metadatos. El driver no mide undo real del documento.

Cada carga añade un índice persistente, construido y compactado una sola vez,
con **512 roundtrips** de inserción `x\n`/inversa: primero inicio y EOF, después
posiciones deterministas dispersas. No se reconstruye ni compacta el índice
entre parejas. Se verifican longitud y entradas locales en cada paso, y todas
las entradas/EOL y búsquedas de control al final contra el vector original.
Las filas `roundtrip512_retained_initial` y `roundtrip512_retained_final`
registran capacidad/bytes lógicos antes y después de las 512 parejas. Llevan
`sample=0`, `batch_size=0`, `elapsed_ns=0`: son descriptores de retención,
**no** tiempos, p95 ni una estimación de coste unitario. Permiten observar
catálogo/payload retenido por fragmentación sin ocultarlo con un shrink final.
El mismo stress se ejecuta también como regresión sobre el oráculo de 8193
entradas, sin emitir filas desde `--check-only` o los tests.

Conteos esperados del CSV crudo para una ejecución completa con ambas
implementaciones:

| MiB | stride bytes | entradas originales por índice |
| --- | --- | --- |
| 1 | 8 | 131073 |
| 1 | 64 | 16385 |
| 4 | 8 | 524289 |
| 4 | 64 | 65537 |
| 8 | 8 | 1048577 |
| 8 | 64 | 131073 |
| 8 | 8192 | 1025 |
| 8 | 65536 | 129 |

Por implementación/carga hay 15 operaciones medidas (construcción, dos
consultas, doce sentidos de mutación), 31 muestras cada una: 465 filas medidas
y cuatro descriptivas, **469 filas**. Las ocho cargas y dos implementaciones
producen **7504 filas de datos** más cabecera: 7440 medidas y 64 descriptivas.
Un filtro de una carga con ambas estructuras produce 938 filas de datos;
con una estructura, 469. Los cinco calentamientos por operación no se emiten.

No se reporta RAM simulada: estos bytes no incluyen allocator, fragmentación,
oráculo, reemplazos, pila, fuentes, historial, cachés ni frontend. Las asignaciones
de verificación también afectan al proceso aunque queden fuera del tiempo.
Ninguna cifra de esta prueba es memoria privada, working set o pico histórico
de Windows; no permite afirmar el objetivo global menor de 50 MiB.

## Ejecución reservada y decisión pendiente

Comandos previstos para el integrador, no ejecutados durante construcción:

```text
cargo run -p jc-app --example line_index_candidates --release --locked --offline -- --check-only
cargo run -p jc-app --example line_index_candidates --release --locked --offline -- 1 8
cargo run -p jc-app --example line_index_candidates --release --locked --offline
```

Sin argumentos se miden las ocho cargas; dos argumentos filtran MiB y stride.
Los strides 8192 y 65536 se admiten solo con 8 MiB, por ejemplo `8 65536`.
Un tercer argumento opcional `flat`, `block`, `flat-first` o `block-first`
selecciona una estructura o controla el orden (por defecto `flat-first`), por
ejemplo `8 8 block-first`. Permite contrastar orden y ejecutar cargas separadas.
`--check-only` ejecuta únicamente oráculos/regresiones. Existen además dos tests
observables del driver para comparar cada implementación con el oráculo.
El módulo plano incluido conserva sus propios tests al compilar el ejemplo
como test; no se copian ni se cambia su código productivo.

La ventana serial deberá conservar identidad del código/binario, toolchain,
perfil y CSV crudo en un destino nuevo sin sobrescribir evidencia anterior;
el driver solo escribe stdout y no administra archivos de medidas. Deben
comprobarse ambas estructuras, las ocho cargas, 31 muestras por operación y
ausencia de fallos de oráculo antes de resumir. Un mensaje inicial de oráculos
correctos no demuestra que la posterior medición completa haya terminado.

La comparación secuencial tiene sesgo de orden potencial; una decisión
estrecha requiere repetir con orden invertido o cargas separadas usando el
tercer argumento. Cualquier futura integración deberá retirar la ruta que
sustituya y revalidar transacciones, rangos, encoding, bytes/EOL e interfaz
nativa; tiempos de metadatos no son latencia de UI ni aceptación física global.

## Cierre del integrador y decisión

Formato, Clippy del ejemplo con warnings denegados, release y **siete tests del
ejemplo** aprobados. Incluyen tests de la referencia plana y oráculos independientes
con miles de entradas, reemplazos/borrados multibloque y reversión acumulativa.
Se midieron las ocho cargas en procesos separados, primero flat-first y después
block-first: **16 capturas completas / 15008 filas**. Los oráculos finalizaron
en todas y las series tienen 31 muestras. Sin compilaciones/fixtures concurrentes.

Retención lógica después de shrink (bytes, iguales en ambos órdenes):

| Fuente / longitud de línea | Plano | Bloques |
|---|---:|---:|
| 4 MiB / 8 B | 2097204 | 1146962 |
| 8 MiB / 8 B | 4194356 | 2293842 |
| 8 MiB / 64 B | 524340 | 286802 |
| 8 MiB / 8192 B | 4148 | 26706 |
| 8 MiB / 65536 B | 564 | 6482 |

La reducción en los corpus densos es de alrededor de 45 %. Las líneas largas
confirman la patología documentada: más headers/asignaciones y mayor consumo del
candidato, aunque los incrementos absolutos de esos casos son ~22 y ~6 KiB.
Tras 512 roundtrips dispersos en el corpus de 8 MiB/8 B, bloques conserva
2314056 B frente a 2293842 B iniciales; plano 4227128 frente a 4194356 B.
No hubo divergencia de entradas/EOL ni se hizo shrink para ocultar fragmentación.

Tradeoffs de tiempo en 8 MiB/8 B, primer/segundo orden:

| Operación | Plano | Bloques |
|---|---:|---:|
| Construcción, p50 ms | 5,934 / 5,819 | 7,233 / 7,106 |
| get de 4096 consultas, p95 del lote µs | 19,9 / 21,5 | 184,3 / 113,3 |
| upper_bound de 4096, p95 del lote µs | 349,5 / 309,9 | 283,2 / 208,3 |
| Insertar frontera al inicio, p95 µs | 2811,7 / 2847,8 | 44,8 / 33,9 |
| Inversa de esa frontera, p95 µs | 1144,3 / 1140,6 | 32,9 / 25,6 |

Construcción y acceso por ordinal son más lentos; desplazamiento por bloques y
retención densa favorecen el candidato. No se mezclan percentiles de lote con
latencia de consulta individual ni se atribuye ese ahorro a la UI real todavía.

Se selecciona el candidato para **integración y medición**, sin aceptar aún el
objetivo global: reemplazar la ruta plana, conservar API/offsets/EOL y revalidar
F1/F2/F3, proyección real y corpus nativo. No habrá dos índices productivos. Este
checkpoint conserva el comparador reproducible antes de retirar la ruta sustituida.

[Resumen validado](benchmarks/M2-index-candidates-summary.csv) conserva cada
orden/tamaño/stride/estructura/operación. Los prefijos
`M2-index-candidates-r{1,2}-{MiB}-{stride}` contienen muestras, checks y metadata
con hashes; por ejemplo [8 MiB denso, orden 1](benchmarks/M2-index-candidates-r1-8-8-samples.csv)
y [orden invertido](benchmarks/M2-index-candidates-r2-8-8-samples.csv).
`scripts/summarize-index-candidates.py` comprobó las 16 capturas antes de resumir.
