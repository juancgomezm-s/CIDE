# S0 — Decoder prestado y memoria reutilizada

Bloque del 2026-11-02 sobre `570a6eb`, rama `m2a/dual-pane-tui`. No incorpora
funcionalidad ni dependencias. Mantiene un único matcher literal para documento
y proyecto, los datos Unicode 17 y el perfil release existente.

## Construcción

`jc-text::DecodedScalars` interpreta un rango prestado sin construir un String,
un vector de offsets por escalar ni una lista de incidencias. Cada elemento entrega
carácter, rango absoluto de bytes y validez. Valida tramos UTF-8 y recorre sus
escalares; las páginas legacy usan las mismas tablas explícitas. `decode` y
`decode_fragment` ahora materializan sus resultados mediante este mismo decoder:
se retiraron los decoders anteriores, no se añadió una interpretación paralela.

Scanner conserva y rellena un Vec de bytes por tarea. Documento lo rellena desde
los segmentos indexados del core; proyecto desde su fuente acotada. Solo se mueve
la cabecera del Vec para prestar sus bytes durante un turno, sin autorreferencias
ni clones de documentos. Al reanudar se conserva el offset consumido; los cuatro
bytes de contexto no se consumen fuera del límite del fragmento. La capacidad se
libera al terminar/cancelar la tarea. No se añade un cache global ni un snapshot
completo del documento.

Dentro del mismo matcher, un escalar ASCII que no puede empezar el patrón se
descarta directamente cuando no hay prefijo activo, después de resolver el match
pendiente. Conserva el carácter previo para palabra/CRLF. Las coincidencias reales,
los prefijos y Unicode siguen usando el mismo KMP y mapa de orígenes; no existe
un motor de búsqueda ASCII alternativo.

Se conservan BOM solo inicial, BOM literal interior, grupos UTF-8 inválidos,
distinción entre incidencias y U+FFFD válida, límites de escalares originales y
CRLF, folding default C+F no Turkic, posiciones escalares, cuotas, deadlines,
revisiones, reemplazo transaccional y cancelación cooperativa. No cambian bytes,
guardado, encoding, colas ni contratos de entrega.

## Comparación local

Windows 11 Pro 10.0.26300, Ryzen 9 8945HS 8/16, Rust 1.85.1 MSVC offline,
`opt-level="s"`, thin LTO y un codegen unit. Una corrida antes y una final después,
sin cambiar ejemplos, corpus, cuotas ni perfil. Cinco calentamientos y 31 muestras,
p50 índice 15 y p95 índice 29. No se aisló toda la carga del sistema ni se midió
pantalla física. Los CSV contienen los agregados emitidos por los ejemplos.

Proyecto: cuatro archivos, **2.097.144 bytes, 64 coincidencias**, mediante el
ejemplo `project_search_bench` sin cambios. Tiempos en µs, p50 / p95:

| Operación | Antes (`570a6eb`) | Después | S1 histórico (`6e07b1e`) |
|---|---:|---:|---:|
| Primer lote | 8262 / 10611 | 4697 / 5044 | 5707 / 7220 |
| Fin | 31225 / 40930 | 17587 / 18649 | 21239 / 27457 |
| Cancelar + join | 44 / 97 | 58 / 104 | 56 / 73 |

El p95 de fin baja **54,4 %** frente al antes contemporáneo y queda **32,1 %**
por debajo de S1 histórico en este corpus. El coste de ~35 % registrado en S0
se recupera en esta carga; no prueba una mejora universal en otros corpus.
La cancelación no mejora: p95 104 µs frente a 97 µs contemporáneos/73 µs S1.
Se cronometra cancelar/destruir tras la primera entrega densa disponible, no
preempción del decoder ni una cola necesariamente llena.

[Antes proyecto](benchmarks/S0-decoder-before-project.csv),
[después proyecto](benchmarks/S0-decoder-after-project.csv),
[S1 histórico](benchmarks/S1-search.csv).

Documento: mismo `document_search_bench`, líneas ASCII de ocho bytes, consulta
ausente, sin snapshot ni worker. Schedule, fin y cancelación usan 31 muestras por
tamaño; slot p95 usa todos los turnos retenidos. Tiempos en µs:

| Tamaño | Fin antes p50 / p95 | Fin después p50 / p95 | Slot p95 antes → después | Cancelar p95 antes → después |
|---|---:|---:|---:|---:|
| 4 KiB | 50 / 59 | 19 / 25 | 57 → 23 | 0 → 0 |
| 1 MiB | 14280 / 14882 | 5441 / 5554 | 262 → 89 | 0 → 0 |
| 4 MiB | 51626 / 58558 | 21330 / 22388 | 219 → 88 | 0 → 10 |
| 16 MiB | 358788 / 632051 | 89093 / 178829 | 650 → 180 | 1 → 7 |

Programar 16 MiB conserva p50/p95 2/3 µs. Cancelar libera ahora el buffer retenido;
no se afirma mejora de esa operación ni se interpreta cero µs como coste nulo.
La dispersión de 16 MiB es alta: el p95 antes contemporáneo fue 632051 µs,
frente a 401671 µs en S0 histórico. El p95 después también excede mucho su p50.
Son tiempos del modelo, no una garantía dura ni tecla-a-pantalla.
[Antes documento](benchmarks/S0-decoder-before-document.csv),
[después documento](benchmarks/S0-decoder-after-document.csv).

Se conserva la primera pasada del bloque:
[proyecto](benchmarks/S0-decoder-first-pass-project.csv) y
[documento](benchmarks/S0-decoder-first-pass-document.csv). El decoder prestado
por sí solo dio p95 de proyecto 30633 µs, pero documento 4 MiB subió a 67852 µs.
Esa regresión motivó el rechazo ASCII sin prefijo y permitir inline en las
fronteras calientes del decoder/matcher. Se midió el código final después de ese
ajuste. No se repitieron suites por cambios documentales.

## Validación y límites

La suite final completa, fmt, Clippy workspace/all-targets con `-D warnings`,
release y los dos recorridos nativos se registran en [VALIDACION](VALIDACION.md).
Las nuevas regresiones comprueban grupos inválidos/truncados, BOM interior,
offsets absolutos, roundtrip de todos los bytes válidos de las tres páginas legacy
y 400 matches plegados a través de turnos/límites 4/5/7/4096, con posiciones y
entregas acotadas. Se conservan los oráculos de folding y matching independientes.

Se elimina la asignación de String/offsets/incidencias en el scan, pero no se midió
un contador global de asignaciones ni RAM privada/picos en este bloque. El Vec
tiene crecimiento de capacidad del allocator; el límite de fragmento no es un
límite de RAM del proceso. Resultados, prefijos y fuentes de proyecto conservan
sus presupuestos anteriores. No se acredita <50 MiB, p95 físico <16 ms, Linux,
GUI, IME/grafemas, regex, AVM ni cierre completo de M2A. El siguiente trabajo es
ampliar corpus Unicode/legacy/frío/líneas extremas/muchas piezas y la matriz de
teclado antes de aceptar rendimiento global. CI remoto continúa reservado a M5.
