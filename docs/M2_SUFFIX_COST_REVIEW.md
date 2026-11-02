# M2-F — Medición del coste de sufijos densos

Construcción del 2026-11-02, posterior al índice compacto documentado en
[M2_DENSE_LINE_METADATA](M2_DENSE_LINE_METADATA.md). El worker preparó el benchmark
opt-in del flujo real sin ejecutarlo. El integrador compiló y ejecutó después
una corrida release serial; los datos de esa base se registran abajo. Esa evidencia
motivó una política explícita de crecimiento en `LineIndex::replace_after`.
La comparación posterior y la validación completa ya están registradas abajo.

La primera entrega fue `crates/jc-app/examples/suffix_cost.rs` y este documento.
El ejemplo ya quedó devuelto al integrador. La continuación modifica únicamente
`crates/jc-app/src/workbench/line_index.rs` y este documento: no cambia manifests,
exportaciones, APIs, core, push/scan frío, shrink ni desplazamiento del sufijo.

## Frontera utilizada

La ubicación inicial propuesta en `jc-text/examples` no permitía un protocolo
correcto: jc-text no depende del core/app y TextProjection es `pub(super)` dentro
de jc-app. El integrador autorizó ubicar el ejemplo en **jc-app**, usando Workbench
público. No se copiaron módulos privados ni se publicó una nueva API de medición.

El ejemplo usa `apply_text_edits` por revisión, `undo`, lectura de posiciones/filas,
`text_statistics` y la sesión/Document prestados de solo lectura. Por esa ruta se
ejecutan realmente validación de fronteras/encoding, transacción, ChangeSet,
rebase/normalización de vistas y actualización de TextProjection. No es una
simulación de desplazar offsets en un vector auxiliar.

## Cargas y posiciones

Fuentes ASCII UTF-8 explícito de **1, 4 y 8 MiB** exactos, repitiendo ocho bytes
`abcdefg\n`. Terminan en LF; el editor representa `N/8 + 1` líneas lógicas,
incluida la línea vacía final. Una ventana de investigación del sufijo no exige
otro corpus Unicode: esas garantías siguen cubiertas por las regresiones F2/M2;
este ejemplo no las vuelve a certificar.

Cada tamaño usa inicio, medio y final. Inicio=byte 0, medio=N/2 (frontera de línea)
y final=EOF. Cada combinación abre una nueva sesión/proyección desde la misma
fixture de ese tamaño; después conserva ese Workbench durante todas sus muestras.

| Operación | Inicio/medio | Final | Cambio esperado de líneas |
|---|---|---|---:|
| insert_scalar | Insertar `x` en el anchor | Insertar `x` en EOF | 0 |
| insert_line | Insertar `x\n` en el anchor | Insertar `x\n` en EOF | +1 |
| delete_scalar | Borrar el primer `a` de la línea | Borrar `g` en N−2, antes del LF final | 0 |
| delete_line | Borrar ocho bytes desde el anchor | Borrar la última línea completa desde N−8 | −1 |

Los offsets exactos se escriben en cada muestra. «Final» en un borrado no pretende
eliminar más allá de EOF. Las operaciones alteran uno/dos/ocho bytes, no sustituyen
el documento entero para restaurar el caso.

## Frío, calentamiento y distribución

Se separan estas fases:

1. `open`, `cold_model`: lectura/fingerprint/apertura de la sesión. El timer incluye
   Workbench::open_file; no incluye generar la fixture ni construir Workbench.
2. `cold_index`: primera consulta de número de líneas. Construye el índice real
   antes de leer layouts. Una muestra por caso.
3. `cold_target_layout`: primera posición del anchor después del índice. Una
   muestra por caso; en EOF la línea lógica es vacía y no equivale al layout de
   siete caracteres de inicio/medio.
4. `warm_move_probe`: cinco calentamientos y 31 muestras. Right+Left en inicio/
   medio; Left+Right en EOF, retornando al mismo offset. Cada muestra consulta
   también posición, número de líneas y una fila visible de hasta 80 columnas.
5. Para cada edición: un ciclo `first_cycle` registrado, cinco ciclos de
   calentamiento descartados y 31 ciclos `warm` retenidos. La primera operación
   **no** es una proyección fría: el índice y el layout objetivo ya existen.

Cada ciclo de edición separa cuatro timers: `apply`, `projection_probe`, `undo`
y `undo_projection_probe`. La propuesta pequeña se construye fuera de `apply`;
su encoding/validación, edición y notificación sí están dentro. Las lecturas
posteriores incluyen número de líneas, posición y una fila visible. Los asserts,
captura de contadores/mapas, checksum y salida de CSV quedan fuera de los timers,
salvo el manejo normal de Result/Option en la propia llamada medida.

Workbench normaliza vistas dentro de apply/undo y eso puede actualizar ya la
proyección. **No se interpreta `projection_probe` como todo el coste incremental**:
las columnas por fase permiten ver dónde ocurrió el trabajo real, sin forzar una
segunda ruta para retrasarlo artificialmente.

Los 31 valores son duraciones de llamadas/ciclos individuales, no medias de 50
operaciones como en el ejemplo histórico F2. El resumen caliente usa p50 en el
índice 15 y p95 en el 29 de las muestras ordenadas. Las fases frías y first_cycle
son muestras únicas, no percentiles. No calcular un factor de mejora mezclando
esos protocolos ni juntar posiciones/tamaños distintos como una distribución.

«Frío» significa índice/layout aún no construido en ese Workbench. No se vacía
cache del SO ni se mide disco/cache de CPU completamente frío. Las fixtures se
escriben antes; el archivo puede estar cacheado. El orden fijo de tamaños y
posiciones tampoco controla frecuencia, temperatura o toda la carga del equipo.

## Datos observables e invariantes

Stdout conserva 31 columnas CSV por observación:

- Tamaño, ubicación, operación, fase, muestra y etapa; rango sustituido, bytes
  insertados y líneas esperadas.
- `estimated_suffix_lines`: líneas completas del corpus inicial tras el final del
  reemplazo. Es una **estimación de la forma de carga**, no un contador de entradas
  que la implementación movió. En lecturas sin edición vale cero.
- Nanosegundos y deltas de bytes escaneados/decodificados de TextProjection.
- Capacidad lógica de proyección, fuentes retenidas, cargo de historial, piezas y
  pasos undo/redo antes/después.
- Revisiones, número de cambios y mapeo del EOF antiguo al tamaño resultante.

`text_statistics` observa contadores/capacidad sin actualizar la proyección; no
oculta trabajo antes del timer. El ejemplo verifica que cada commit y undo publica
su ChangeSet, que sus revisiones coinciden con la transición y que EOF mapea al
EOF resultante. Tras undo se recuperan revisión, tamaño y número de líneas.

Al terminar un caso se calcula un checksum sobre `Document::chunks()` prestados,
comparándolo con el checksum obtenido durante la escritura de la fixture. Se
recorre todo el contenido una vez fuera de las muestras, sin convertirlo a Vec
ni decodificarlo. No se clona Document/FileSession/historia por iteración.

La fuente se genera con un bloque temporal de 64 KiB. Solo se crean archivos
nuevos dentro de una carpeta temporal propia; su guard borra esos archivos
enumerados y después la carpeta vacía, sin eliminación recursiva. No se guarda
ninguna edición del Workbench ni se escriben fuentes del usuario.

## Hallazgos y relación con los datos

- `LineIndex::shift_tail` recorre los offsets de la cola. Compactar de nueve a
  cuatro bytes por línea reduce capacidad, pero no elimina ese trabajo O(L).
  Inicio/medio/final y tres tamaños permiten observar cómo cambia la ruta real.
- `replace_after` usaba splice sin reservar el tamaño final. La primera inserción
  de newline confirmó el crecimiento geométrico del contenedor y su retención
  tras undo; first_cycle lo distingue de los ciclos calientes. No se presume
  que cada edición provoque una nueva expansión.
- `TextProjection::ensure` escanea una vecindad del cambio y aplica mapas directos/
  inversos, pero también desplaza layouts/checkpoints de la cola. Bytes fuente
  visitados pequeños **no** prueban que los metadatos actualizados sean pocos.
- Validación de fronteras, normalización de vistas, metadatos de piezas y undo
  comparten el timer de apply/undo. El benchmark mide esa ruta, no un tiempo puro
  de `shift_tail`. Piezas/historial se publican para detectar un cambio de forma.
- Los tamaños de este corpus permanecen en la representación compacta; no miden
  promoción de 30 bits a offsets nativos ni documentos de más de 1 GiB.

Los datos no justifican sustituir el índice por otro árbol ni conservar un segundo
índice productivo. La corrección se limita a controlar la capacidad solicitada
en un punto de crecimiento ya identificado.

## Base medida por el integrador

Corrida serial del 2026-11-02, Windows 11 10.0.26300, Rust 1.85.1 MSVC release,
locked/offline, sobre `6cf6f13`. El runner validó **4914 observaciones de 31
columnas y 153 resúmenes calientes**. Cada resumen contiene 31 muestras; las
fases frías y first_cycle siguen siendo muestras únicas. La metadata conserva
SHA-256 del ejemplo/binario y el alcance exacto, sin medición de RAM del proceso.
Clippy/build pasaron antes de la medición; este worker no ejecutó la corrida.

En 8 MiB, tiempos calientes p95 en microsegundos:

| Ruta | Inicio | Medio | EOF |
|---|---:|---:|---:|
| Movimiento de ida/vuelta + probe | 2,9 | 2,7 | 1,6 |
| Insertar `x\n`, apply | 601,8 | 328,1 | 1,9 |
| Undo de `x\n` | 1135,8 | 584,3 | 1,2 |

Insertar `x` al inicio dio p95 527,3 µs en apply. Las etapas de edición visitaron
como máximo 20 bytes de scan y 63 de decode por observación. Esto es compatible
con una vecindad de fuente pequeña y un sufijo de metadatos todavía costoso: no
se confunden los contadores de bytes fuente con entradas de índice desplazadas.
La diferencia por posición pertenece al flujo Workbench completo, no a una
medición aislada de `shift_tail`.

Primera inserción de newline al inicio, una observación por tamaño:

| Tamaño | Apply µs | Proyección lógica antes B | Después B |
|---|---:|---:|---:|
| 1 MiB | 161,1 | 524468 | 1048760 |
| 4 MiB | 796,1 | 2097332 | 4194488 |
| 8 MiB | 1760,8 | 4194484 | 8388792 |

El crecimiento casi duplicó la capacidad lógica de la proyección por añadir una
línea. En 8 MiB, el CSV confirma 8388792 B también después de undo; el índice no
se reduce automáticamente al quitar esa línea. **No es RAM privada ni peak
commit**. Los tiempos first_cycle no son p95 y no se mezclan con la tabla caliente.

Datos base: [muestras](benchmarks/M2-suffix-cost-first-samples.csv),
[percentiles](benchmarks/M2-suffix-cost-first-percentiles.csv),
[metadata/hashes](benchmarks/M2-suffix-cost-first-metadata.json).

## Política de crecimiento construida después de medir

Antes del splice, `replace_after` calcula con aritmética comprobada el número
final de registros: longitud anterior menos rango sustituido más los registros
nuevos (excluido el primer registro de contexto del scanner). Solo cuando esa
longitud no cabe se pide capacidad explícita mediante `reserve_exact`.

La solicitud agrega una holgura de aproximadamente **1/32 de los registros
finales**, con mínimo práctico de 16 registros y máximo de **32 KiB lógicos por
índice**. El máximo se convierte a registros según la representación activa:
cuatro bytes en Compact; `size_of::<usize>() + 1` en Wide, nueve en este Windows
de 64 bits. Wide cobra ambos vectores juntos y los reserva antes de splicear.
No son 32 KiB adicionales para cada vector.

La holgura permite reutilizar capacidad durante ráfagas; no se pide exactamente
un registro por cada tecla. Tampoco se duplica una tabla grande para acomodar una
sola línea. El cálculo respeta el límite representable de cada asignación Vec
y usa sumas/restas comprobadas; los offsets de bytes siguen siendo usize y la
promoción a Wide permanece lossless, sin truncar una coordenada grande.

Hay un solo índice y una representación activa. No cambian el scan/push de
construcción inicial, `shift_tail`, el formato compacto ni la política explícita
de shrink. Un borrado/undo no reduce capacidad ni corrige una capacidad excesiva
ya existente: el tope describe la **holgura solicitada en un crecimiento**, no
un límite universal de capacidad ociosa después de borrar muchas líneas.
`reserve_exact` tampoco constituye una garantía de RAM física del allocator.

Tradeoff: limitar holgura reduce retención, pero un burst largo puede requerir
varias reallocaciones/copias del índice al agotar esa reserva. No se promete
complejidad amortizada constante para crecimiento ilimitado con un tope fijo.
Los desplazamientos O(L), splice de límites locales, promoción y OOM del allocator
siguen existiendo. No se cambió perfil release ni se declara mejora general de
latencia antes de comparar la corrección con la misma carga.

Tres regresiones nuevas quedaron construidas en `line_index.rs`: crecimiento
de una frontera e inversión preservando todos los offsets/EOL en Compact/Wide;
ráfagas que reutilizan capacidad y mantienen sus entradas; promoción por reemplazo
con offset hasta usize::MAX sin pérdida. Comprueban límites observables de memoria
lógica y reutilización, no una capacidad objetivo exacta del algoritmo. Los offsets
Wide son metadatos sintéticos; no se abrió un archivo real de ese tamaño.

El worker aplicó rustfmt y revisión estática; el integrador validó después el
conjunto y repitió el protocolo completo, sin compilar durante la medición.

## Comparación posterior y cierre local

240 pruebas workspace aprobadas (dos helpers ignorados invocados por sus padres),
fmt, Clippy workspace/all-targets con warnings denegados y release. El mismo
ejemplo, con idéntico hash fuente, produjo otras 4914 observaciones / 153 resúmenes
calientes. Se conservan [muestras posteriores](benchmarks/M2-suffix-cost-after-samples.csv),
[percentiles](benchmarks/M2-suffix-cost-after-percentiles.csv) y
[metadata/hashes](benchmarks/M2-suffix-cost-after-metadata.json). Se verifica también
el hash del índice productivo y del binario; la base `6cf6f13` precede a la política.

Primera inserción de newline al inicio, capacidad lógica de la proyección:

| Corpus | Antes de editar, ambas corridas | Tras editar, política anterior | Tras editar, política acotada |
|---|---:|---:|---:|
| 1 MiB | 524468 B | 1048760 B | 540860 B |
| 4 MiB | 2097332 B | 4194488 B | 2130104 B |
| 8 MiB | 4194484 B | 8388792 B | 4227256 B |

En 8 MiB se retienen 4161536 bytes menos de capacidad de proyección después de esa
edición, alrededor de 3,97 MiB. No son bytes de RAM privada medidos ni una mejora
del pico nativo. El scan de ese paso sigue en 6 B y decode en 21 B; los máximos
por etapa de edición del corpus permanecen en 20/63 B, respectivamente.

No se declara aceleración general. La primera inserción observada tardó
161,1→161,8 µs (1 MiB), 796,1→625,6 µs (4 MiB) y **1760,8→2145,2 µs (8 MiB)**.
Son muestras únicas; el aumento en el caso de 8 MiB se conserva y motiva ampliar
la distribución fría, sin ocultarlo tras los percentiles calientes.

En 8 MiB/inicio, p95 caliente apply de newline: 601,8→574,2 µs; undo:
1135,8→1131,4 µs. Movimiento+consulta caliente posterior: 3,1/3,0/1,5 µs para
inicio/medio/EOF. La variación entre corridas no demuestra una mejora estadística;
el coste O(L) persiste y no se acepta pantalla física por estas llamadas del modelo.
La decisión acepta ahorro lógico observado con el tradeoff de realocaciones por
ráfagas y coste frío todavía por distribuir.

## Ejecución opt-in y límites

En la ventana serial del integrador, con el mismo toolchain/perfil y sin workers
compilando o ejecutando fixtures:

```console
cargo build -p jc-app --example suffix_cost --release --locked --offline
target\release\examples\suffix_cost.exe > suffix-raw.csv 2> suffix-summary.csv
```

El wrapper local debe fijar TEMP/TMP a `work/test-temp` como en las otras cargas
del equipo. No se necesita Git, Progress, navegador, nuevas dependencias ni red.
La segunda llamada usa el binario ya construido: así stderr contiene solamente
el resumen, sin mensajes de compilación Cargo. Se permite filtrar con argumentos
posicionales, por ejemplo `suffix_cost.exe 4 middle`; la corrida completa sin
argumentos cubre las nueve combinaciones.

En la primera entrega este worker aplicó rustfmt al ejemplo y revisó estáticamente
el formato CSV (31 columnas). El integrador ejecutó la base y la comparación
posterior al cerrar el bloque completo; no se repitieron suites por cada retoque.

Capacidad lógica y cargo de historial **no son RAM privada, RSS, peak commit ni
contadores de asignación**. No se instrumenta un allocator en este ejemplo. Tampoco
incluye composición/VT/pantalla, GUI, IME/grafemas/DPI, Linux, servicios o aceptación
global M2A. Las metas de RAM y latencia física conservan los pendientes del plan.
