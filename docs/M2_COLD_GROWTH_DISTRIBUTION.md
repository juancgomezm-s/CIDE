# M2-F — Distribución de la primera inserción de newline

Construcción opt-in del 2026-11-02. Archivos de este frente:
`crates/jc-app/examples/cold_growth.rs` y este documento. No cambia APIs,
dependencias, manifests, el índice ni su política de crecimiento. Este worker
no ejecutó Cargo, fixtures ni la medición. El integrador revisó/compiló el conjunto
y completó después tres pares de capturas seriales sobre ambas versiones.

## Pregunta y límite de comparación

[M2_SUFFIX_COST_REVIEW](M2_SUFFIX_COST_REVIEW.md) separó first_cycle de las 31
muestras calientes. La primera inserción de una línea reveló crecimiento
geométrico de la capacidad; después, el índice pasó a reservar holgura explícita
acotada. Esos tiempos first_cycle eran **una observación por caso**, no una
distribución. Tampoco deben ocultarse las variaciones de esa única observación.

El nuevo ejemplo mide una distribución de esa primera edición, después de crear
índice/layout en **cada Workbench nuevo**. No permite calcular un factor de mejora
contra los tiempos individuales del ejemplo anterior: cambia el protocolo y
antes de la primera edición ya no se ejecutan las ráfagas de insert_scalar de
suffix_cost. Una comparación futura de versiones debe correr este mismo ejemplo
con binarios identificados y cuotas/perfiles iguales en ambos lados.

## Casos y sesiones

Una fixture por tamaño: 1, 4 y 8 MiB exactos de `abcdefg\n`, UTF-8 explícito,
sin BOM, con LF final. Se crea mediante un bloque de 64 KiB y se obtiene su
checksum durante esa escritura. No se genera un Vec del documento por muestra.

Cada tamaño se ensaya en dos posiciones: byte 0 y EOF. Por combinación se descarta
una sesión completa de calentamiento y se retienen **31 sesiones independientes**.
Cada sesión abre su fuente en un Workbench nuevo; los bytes le pertenecen mediante
el flujo real FileSession/Document. No se clona una sesión/documento/historial ni
se restablece una muestra copiando todo el contenido en el buffer.

La carga completa abre 192 sesiones en serie (6 casos × 32), reteniendo datos de
186. Solo existe un Workbench de ensayo a la vez. Esa apertura nueva sí necesita
leer/poseer los bytes reales de cada sesión: no se presenta como una operación
de cero lecturas. El archivo de cada tamaño se reutiliza entre sesiones.

«Cold» se refiere al estado de índice/layout de esa instancia. No se vacía la
caché del filesystem, heap o CPU ni se reinicia el proceso por cada sesión. El
archivo acaba de escribirse y puede estar en caché. El orden fijo de tamaños/
posiciones y la vida del allocator limitan la interpretación de sus percentiles.

## Cinco fases medidas

| Fase | Operación dentro del timer |
|---|---|
| `open` | Workbench::open_file y su lectura/probe/fingerprint; Workbench::default queda fuera |
| `index` | Primera consulta de line_count_for_tab, construyendo el índice de líneas |
| `layout_and_caret` | Primera posición del anchor y jump_to_location para preparar la vista objetivo |
| `first_apply` | Un apply_text_edits de `x\n` en el anchor, con revisión esperada |
| `undo` | Un undo que restaura el contenido/revisión inicial |

La propuesta pequeña de dos bytes se reserva antes de first_apply. Ese timer sí
incluye el camino real de encoding/validación, transacción, mapa, rebase y
normalización de vistas/proyección. No se extrae una copia de TextProjection ni
se mide un vector auxiliar. Las APIs de lectura son las existentes de Workbench;
el contador de estado solo observa text_statistics y metadatos del documento.

En EOF, el primer layout pertenece a la línea lógica vacía posterior al LF final;
no equivale al layout de siete caracteres del inicio. El timer de layout incluye
preparar el caret, por lo que tampoco se mezcla con el cold_target_layout del
ejemplo previo como si ambos protocolos fueran idénticos.

Después de cada fase se toman contadores/capacidades fuera del timer. Tras apply
y undo se comprueba el número de líneas: el ejemplo exige que esa verificación
no incremente scanned/decoded/capacidad de proyección. Así no oculta una eventual
actualización diferida costosa en código de verificación fuera de las muestras.

## Verificación prestada

Una inserción crea exactamente dos bytes y una línea lógica adicional. El ejemplo
comprueba el ChangeSet directo, sus revisiones, una edición publicada y el mapeo
del anchor y EOF antiguos hacia los nuevos. En inicio observa mediante
range_chunks el prefijo `x\nabcdefg\n`; en EOF observa el sufijo
`abcdefg\nx\n`. No materializa el documento para esas comprobaciones.

Undo debe publicar su mapa inverso, restituir revisión/tamaño/líneas y dejar la
sesión sin modificación. Después de **todos los timers de esa sesión** se recorre
Document::chunks prestado para comparar checksum, cantidad de bytes, LF, ausencia
de CR y extremos originales. Este recorrido completo de verificación no copia ni
decodifica el documento. Sí toca sus páginas/cachés antes de liberar la sesión;
esa limitación está incluida en la descripción de frío del protocolo.

El checksum es FNV-1a de verificación de fixture, no una identidad criptográfica ni
un sustituto de los SHA-256 del ejemplo/binario que el runner debe conservar.
No hay guardado de la edición ni acceso a fuentes del usuario. Las fixtures se
crean con create_new en carpeta temporal propia y se eliminan por lista explícita
de archivos, seguida de remove_dir no recursivo.

## Salida y percentiles

La corrida completa emite **930 filas raw de 27 columnas**: 6 casos × 31 sesiones
× 5 fases. Conserva tamaño, posición, número de sesión, fase, anchor, líneas
esperadas, nanosegundos, deltas scanned/decoded, capacidad lógica de proyección/
fuentes e historial, piezas, pasos undo/redo, revisiones, tamaños y mapa de EOF.

Stderr emite 30 resúmenes, uno por caso/fase, con 31 tiempos ordenados: p50 índice
15 y p95 índice 29. Son percentiles de fases de instancias nuevas, no medias por
lote ni una muestra first_cycle presentada como p95. Los datos de calentamiento
no se mezclan con los retenidos y no se agrupan tamaños/posiciones distintos.

La salida se compone al final, fuera de los timers. El programa conserva solo
registros de medición y no los documentos de sesiones anteriores. No se inicia
Git, Progress, un navegador, otro frontend ni un indexador de proyecto.

## Ejecución posterior por el integrador

Cuando termine la construcción de los frentes paralelos, reservar una única
ventana sin compilaciones, fixtures ni benchmarks de otros workers:

```console
cargo build -p jc-app --example cold_growth --release --locked --offline
target\release\examples\cold_growth.exe > cold-growth-raw.csv 2> cold-growth-percentiles.csv
```

Fijar TEMP/TMP en `work/test-temp` mediante el entorno local ya usado. Ejecutar el
binario construido para que los mensajes Cargo no contaminen stderr del CSV.
Sin argumentos recorre los seis casos; `cold_growth.exe 4 start`, por ejemplo,
filtra un tamaño/posición. El runner debe guardar OS, toolchain/perfil, commit,
hash de código/binario y protocolo junto a las muestras.

El worker aplicó rustfmt solo al archivo nuevo y revisó estáticamente la forma
CSV; su entrega inicial no contenía resultados ejecutados. El integrador completó
después la compilación y las capturas registradas a continuación.

## Cierre medido — mismo protocolo en las dos versiones

Formato, Clippy del ejemplo con warnings denegados, build release y ejecución
completos. No se repitió la suite funcional de 240 pruebas: no hubo cambios
productivos en este bloque. Se exportó `6cf6f13` mediante git archive a una carpeta
nueva bajo work, se añadió allí **exactamente el mismo ejemplo** y se compiló
offline con Rust 1.85.1 MSVC y target aislado. El árbol productivo permaneció en
`d6de969`. Sus hashes de código/binario están en los metadata; los seis hashes
del ejemplo coinciden. No se copiaron documentos vivos ni proyecciones.

Tres pares de procesos, en orden anterior/actual, actual/anterior y anterior/actual.
Cada captura conserva 930 filas / 30 resúmenes, total **5580 observaciones** de
1116 sesiones retenidas (1152 contando calentamientos). Sin compilaciones/fixtures
concurrentes, sin purgar cachés ni atribuir a «cold» un estado físico de disco/CPU.

8 MiB, inserción inicial, tiempos de first_apply en microsegundos:

| Par | Anterior p50 / p95 | Actual p50 / p95 |
|---|---:|---:|
| 1 | 1150,7 / 1424,0 | 1151,5 / 2361,9 |
| 2 | 2021,9 / 3688,9 | 1874,9 / 3399,4 |
| 3 | 1982,3 / 3858,1 | 2041,5 / 3251,5 |

La mediana del primer par es prácticamente igual y su cola actual mayor; los
otros pares no repiten ese signo en p95. Ambas versiones varían entre capturas.
Estos datos **no confirman una regresión de latencia consistente ni una mejora
general**; tampoco permiten atribuir la variación a una causa del sistema concreta.
El caso único anterior 1760,8→2145,2 µs se conserva en M2_SUFFIX_COST_REVIEW y
no se reemplaza por un percentil favorable. El coste lineal del sufijo permanece.

El ahorro de capacidad es reproducible en estas sesiones; la memoria nativa se
evalúa por separado en [M2_DENSE_EDIT_EVIDENCE](M2_DENSE_EDIT_EVIDENCE.md).
No se acepta la puerta global/física a partir de los tiempos del modelo.

Datos: [base inicial](benchmarks/M2-cold-growth-baseline-samples.csv),
[actual inicial](benchmarks/M2-cold-growth-current-samples.csv),
[base par 2](benchmarks/M2-cold-growth-baseline-r2-samples.csv),
[actual par 2](benchmarks/M2-cold-growth-current-r2-samples.csv),
[base par 3](benchmarks/M2-cold-growth-baseline-r3-samples.csv),
[actual par 3](benchmarks/M2-cold-growth-current-r3-samples.csv).
Cada prefijo conserva también percentiles y metadata; el
[resumen comparado](benchmarks/M2-growth-cold-comparison.csv) deriva de las filas
crudas y mantiene par/tamaño/posición/fase separados.

`scripts/summarize-growth-evidence.py` valida filas/casos/hash de ejemplo y los
recorridos nativos antes de resumir. Rehúsa sobrescribir salidas; para recalcular:

```console
python scripts/summarize-growth-evidence.py --output-dir work/growth-summary-repeat
```

Capacidad lógica no es RAM privada/working set/peak commit ni un contador de
asignaciones del allocator. No se instrumenta RAM aquí; el frente ConPTY denso
se mide por separado. Tampoco se acredita disco físicamente frío, latencia VT o
pantalla, IME/DPI/grafemas/Linux/GUI ni aceptación global de M2A. La política de
crecimiento actual no cambia durante este experimento.
