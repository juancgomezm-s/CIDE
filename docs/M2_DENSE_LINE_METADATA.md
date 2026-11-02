# M2 — Metadatos compactos de líneas

Bloque posterior a `4f14da9`, sobre el caso de ocho documentos de líneas cortas
que excedía el objetivo exploratorio de RAM. Conserva la corrección del layout
frío de `1d6bca9`, el decoder/matcher únicos y los presupuestos existentes.

## Representación y migración

La proyección almacenaba un `usize` para el inicio de cada línea y un `u8` para
su EOL: nueve bytes por línea en este Windows de 64 bits, sin contar capacidad
sobrante. Ocho fuentes de 4 MiB con líneas de ocho bytes retienen millones de
entradas aunque solo se dibujen dos viewports. El cache de layouts de 1 MiB no
incluye esos metadatos globales de línea.

`workbench::line_index::LineIndex` sustituye ambos vectores por un índice único:

- Cuatro bytes por línea cuando el offset absoluto cabe en 30 bits: offset en
  los bits superiores y EOL en los dos inferiores (0 sin EOL, 1 LF, 2 CRLF, 3 CR).
- Offsets públicos del componente siguen siendo `usize` de bytes. Si un offset
  almacenado o la cota de una actualización supera 1.073.741.823, promueve a offsets nativos con
  EOL separado, preservando todo el rango sin casts truncados. No hay un nuevo
  límite de tamaño del documento ni una promoción inversa implícita en cada tecla.
- El enum posee una representación a la vez. No mantiene dos índices espejo ni
  cambia el almacenamiento canónico del core, los snapshots o las vistas.
- Búsqueda de línea sigue siendo binaria. El escaneo EOL conserva LF/CR/CRLF,
  BOM y la línea lógica vacía al EOF. El scan sabe que el último registro es final
  y reserva solo ese registro, evitando duplicar la capacidad para añadir la
  línea vacía de un archivo que termina en newline.
- Actualizaciones directas/inversas desplazan offsets del sufijo y reemplazan
  límites locales. El EOL de la cola continuada se conserva al unir entradas;
  los contadores globales de tipos EOL se actualizan desde esos mismos cambios.

No se fusiona este índice con las líneas del DiffModel: aquel es un read model
inmutable con terminadores significativos y cero líneas para fuente vacía;
la proyección de edición representa una línea lógica vacía. Son contratos distintos.

La mejora de representación no elimina los desplazamientos O(L) del sufijo,
la promoción grande, el recorrido inicial, la recolección de fuentes ni los
picos del allocator. Los bytes de `text_statistics` siguen siendo capacidad
lógica de índice/layout; el consumo del proceso se mide por separado.

La primera pasada reveló otra copia evitable: `reclaim_unused_storage` reconstruía
incluso una arena cuyos bytes seguían alcanzables en su totalidad. Ahora conserva
esa arena y solo ajusta capacidad sobrante; la arena con huecos sigue recolectándose
y reubicando sus intervalos. No descarta undo/redo ni altera revisión, estado guardado
o mapas. Una nueva regresión conserva la arena original mientras libera una rama
descartada en Added y recorre los estados guardados mediante undo/redo.

## Método de comparación

Se conservaron los ejecutables anteriores antes de compilar la corrección,
sin checkout/reset del árbol ni reinstalar toolchain. Sus hashes y los finales
se registran con el runner. Se reutiliza el mismo ejemplo instrumentado
`m2_acceptance`: ocho corpus, un proceso por corpus, fases frías individuales y
cinco calentamientos más 31 muestras de cada operación caliente. El allocator de
medición solo se enlaza al ejemplo, nunca al editor.

`scripts/measure-m2-dense.py before|after` compara ambos ejecutables en una ventana
serial estable y conserva las muestras crudas. El runner ConPTY también admite
`dense-before|dense-after` y una ruta explícita de ejecutable dentro del proyecto:
ocho documentos ASCII de 4 MiB con líneas de ocho bytes y una primera línea de
fixture larga para cronometraje de Right. Cada forma usa un calentamiento y
siete procesos retenidos, con dos vistas y visita de los ocho documentos.

Las compilaciones/fixtures de workers permanecieron detenidas durante la medición.
La llamada de visualización fue interrumpida y su kernel reseteado; el worker no
tenía llamadas pendientes ni servidor propio. Un lanzamiento de Edge no devolvió
PID/ventana verificable, por lo que no se cerró una superficie no atribuible ni
se afirma aislamiento completo de procesos ajenos.
Se separan modelo instrumentado, private/working set/peak commit de Windows,
host/ConPTY/controlador y latencia hasta VT. No se mide pantalla física ni se
vacía la caché del sistema. El perfil release y las cuotas no cambian.

## Resultado medido

Windows 11 Pro 10.0.26300, Ryzen 9 8945HS 8/16, Rust 1.85.1 MSVC, perfil de tamaño
existente, offline. No se vació caché del SO ni se aisló toda la carga del equipo.
Cada proceso del modelo prepara/borra solo sus fixtures. Frío/GC/cierre tienen
una muestra por corpus; los tiempos de esas fases no son p95 aunque el resumen
genérico emita el mismo valor en sus columnas de percentiles.

Ocho documentos de 4 MiB, líneas de ocho bytes, modelo instrumentado:

| Medida | Antes | Después |
|---|---:|---:|
| Capacidad lógica de proyección activa tras frío | 4.718.793 B | 2.097.348 B |
| Bytes vivos totales del allocator tras frío | 71.314.383 B | 50.342.823 B |
| Private del proceso tras frío | 73.379.840 B (70,0 MiB) | 51.712.000 B (49,3 MiB) |
| Peak commit del proceso, vida completa | 81.248.256 B (77,5 MiB) | 52.817.920 B (50,4 MiB) |
| Frío de ocho proyecciones, muestra única | 90,46 ms | 104,13 ms |
| Right+posición+viewport p50/p95, 31 muestras | 52,0 / 56,9 µs | 52,3 / 87,2 µs |
| Búsqueda ausente completa p50/p95 | 41,457 / 42,470 ms | 41,314 / 43,195 ms |
| Edit/undo/redo/undo p50/p95 | 970,4 / 992,3 µs | 987,7 / 1052,7 µs |
| Asignación solicitada al liberar historial | 33.555.648 B | 1024 B |
| Liberar historial de ocho, muestra única | 4634,2 µs | 3,4 µs |
| Private después de cerrar todos | 1.216.512 B | 1.212.416 B |

La capacidad se reduce cinco bytes por línea en la ruta compacta. No se afirma
una mejora de latencia general: la fase fría fue más lenta y los tails calientes
variaron. En los ocho corpus el Right+posición+viewport p95 final fue 57–103 µs,
todavía bajo el presupuesto exploratorio de 2 ms del modelo, sin certificar UI.
Los desplazamientos de metadatos O(L) y el allocator siguen siendo costes reales.

En la línea Unicode de 16 MiB, la recolección deja de solicitar otros ~16 MiB:
16.777.364 → 128 bytes en esa fase. Peak commit de vida del modelo pasó de
35.012.608 a 18.194.432 bytes. Los casos con huecos genuinos todavía necesitan
reubicación y pueden tener picos de copia; no se elimina toda copia del core.

Ejecutable real, ocho documentos/dos vistas, siete procesos retenidos por lado:

| Medida nativa | Antes | Después |
|---|---:|---:|
| Editor peak commit máximo | 75.382.784 B (71,9 MiB) | 54.226.944 B (51,7 MiB) |
| Editor private muestreado máximo | 75.288.576 B | 54.206.464 B |
| Máximo de private editor+host+ConPTY en un ciclo | 77.672.448 B (74,1 MiB) | 56.598.528 B (54,0 MiB) |
| Host private máximo después | — | 745.472 B |
| ConPTY private máximo después | — | 1.650.688 B |
| Controlador C# private máximo después, aparte | — | 25.886.720 B |
| Arranque observado p50/p95, n=7 | 103,30 / 114,27 ms | 107,37 / 114,43 ms |
| Right inyección-a-VT p50/p95, n=217 | 15,529 / 16,129 ms | 15,539 / 16,086 ms |
| Private máximo observado en fase closed | 2.412.544 B | 1.839.104 B |

El pico del editor bajó ~28 %, pero **51,7 MiB aún supera 50 MiB**; el total con
auxiliares tampoco pasa esa puerta. El p95 de arranque es el máximo de siete
muestras y no demuestra arranque caché-frío. VT no mide la pantalla física.
Las muestras labeled retained de estos CSV incluían también el cierre progresivo
de tabs: no se usan como percentiles de RAM estable. El runner ahora separa closing
para futuras corridas; máximos/peak commit y los tiempos aquí publicados no cambian.

Datos crudos: [modelo antes](benchmarks/M2-dense-before-model.csv),
[modelo final](benchmarks/M2-dense-after-model.csv),
[resumen antes](benchmarks/M2-dense-before-model-summary.csv),
[resumen final](benchmarks/M2-dense-after-model-summary.csv),
[identidad de binarios antes](benchmarks/M2-dense-before-binaries.csv),
[identidad final](benchmarks/M2-dense-after-binaries.csv),
[nativo antes: tiempos](benchmarks/M2-evidence-dense-before-native-timings.csv),
[después: tiempos](benchmarks/M2-evidence-dense-after-native-timings.csv),
[antes: memoria](benchmarks/M2-evidence-dense-before-native-memory.csv),
[después: memoria](benchmarks/M2-evidence-dense-after-native-memory.csv).

Se conserva la [primera pasada](benchmarks/M2-dense-first-pass-model.csv) y su
[identidad](benchmarks/M2-dense-first-pass-binaries.csv), con índice compacto pero
sin evitar copia de arenas completas. Esa medición motivó la corrección del core.
No se mezclan sus métricas con el después final ni se borran resultados intermedios.
Para resumir: `python -X utf8 scripts/summarize-m2-evidence.py --dense`.

## Controles y límites

El bloque integrado de memoria y teclado pasó **209 pruebas, 0 fallos**, fmt,
Clippy workspace/all-targets -D warnings y release. El alias de tipo pedido por
Clippy en la recolección no cambia comportamiento; se repitió el control fallido,
sin repetir las suites inalteradas por ese ajuste. Las regresiones comprueban
promoción de offsets sin truncar, unión de EOL, mapas incrementales/inversos,
historial guardado y rama descartada. La promoción se prueba con metadatos
sintéticos, no abriendo un archivo real de más de 1 GiB.

La [auditoría de teclado](M2_KEYBOARD_MATRIX_AUDIT.md) agregó seis casos y encontró
un menú que despachaba items deshabilitados. El dispatcher TUI ahora comprueba
el registro antes de cualquier efecto; tecla, palette y menú comparten esa puerta.
Se reconfirmaron ambos recorridos nativos existentes de búsqueda/reemplazo/undo:
[documento](benchmarks/M2-dense-document-conpty.json),
[proyecto](benchmarks/M2-dense-project-conpty.json).

Los binarios identificados en el benchmark corresponden a la ventana RAM previa
a integrar la corrección de menú. Los recorridos funcionales finales incluyen
esa corrección; no se repiten medidas de RAM sin un cambio material de esa ruta.
El proveedor Git todavía se integra/valida en su bloque propio y no estaba activo
en estas cargas. No se atribuyen al editor costes de un servicio de lenguaje apagado.

Abiertos: picos >50 MiB, desplazar sufijos densos, fuentes con huecos grandes,
cargas/máquinas adicionales, frío distribuido, pantalla física, IME/DPI/Linux,
revisión visual de la escena y aceptación completa M2A. Sin CI remoto/push,
instalaciones ni cambios a seguridad/configuración/servicios/Qwen/GPU.
