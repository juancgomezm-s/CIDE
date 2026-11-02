# F2 — Acceso por rangos, proyección textual y memoria

Checkpoint implementado sobre `ddc9d41`, rama `m2a/dual-pane-tui`. Mantiene la propiedad de vistas y las transacciones de F1. La base de F2 queda validada localmente; las metas de RAM del proceso, arranque y latencia tecla-a-pantalla siguen abiertas.

## Qué cambió

- `jc-core`: vector de piezas con índice de prefijos por bloques de 128. `range_chunks` ubica la primera pieza en O(log P) y entrega segmentos prestados; `read_range` copia únicamente el rango solicitado. Reconstrucción y comparación de rangos de edición también usan el índice. Las transacciones siguen construyendo metadatos nuevos, sin copiar el archivo para implementar undo.
- `DocumentRead`: lectura prestada de una revisión. El préstamo impide mutar/recolectar el documento mientras se usa. No es todavía un snapshot propio que un worker pueda conservar mientras la UI edita: ese ownership y su cuota corresponden a F3.
- `jc-app`: proyección compartida por documento, con revisión y encoding. Conserva inicios de línea, tipo de terminador, contadores EOL y layouts escalares dispersos. Consulta posiciones/columnas/vecinos sin reconstruir `TextIndex` para cada tecla.
- Los `ChangeSet` directos e inversos actualizan eventos EOL del intervalo modificado con contexto CRLF, desplazan metadatos del sufijo e invalidan/migran solo layouts afectados. Una edición dentro de una línea larga actualiza checkpoints cercanos, conservando los restantes. Las conversiones siguen siendo por escalares; grafemas y ancho visual completo continúan pendientes.
- Caché de layouts con cuota de **1 MiB por documento** contabilizando capacidades de sus contenedores y checkpoints. Los checkpoints adaptan su separación para líneas grandes. El índice estructural de líneas se mide aparte (aproximadamente 9 bytes por línea, más capacidad/alineación), no se confunde con texto decodificado cacheado.
- Apertura: FileSession realiza probe y fingerprint, moviendo el Vec leído al documento; no decodifica texto completo sólo para obtener metadatos de encoding ni clona ese Vec. Workbench obtiene el offset BOM de apertura/reapertura leyendo hasta tres bytes. El benchmark de modelo usa documentos nuevos en memoria y no mide esta apertura de disco.
- TUI solicita filas y columnas visibles; el panel de resultados formatea únicamente sus filas visibles. Se retiraron las rutas `active_decoded`/`decoded_for_tab` y el agrupamiento completo de líneas del renderer productivo. El helper anterior sobrevive exclusivamente como referencia de test.
- Copiar/duplicar, comentarios, sangría e intercambio de líneas leen solo su selección o intervalo de líneas. Enter usa los contadores EOL del índice. Los bytes de BOM literal a mitad del archivo e incidencias de decoder mantienen offsets absolutos; interpretar un fragmento no lo convierte en otro inicio de archivo.

El render aún llama a reveal y el terminal aún limpia la pantalla completa: F3 separará actualización/presentación y emitirá diffs. Search de documento todavía realiza un escaneo explícito síncrono al reconstruir su consulta; resultados/cancelación/jobs acotados siguen perteneciendo a Search Plus/F3. No hay indexador residente de proyecto.

## Historial y recuperación

Además de los 1024 pasos configurables, el core aplica un presupuesto predeterminado de **8 MiB por documento** a historial lógico: metadatos ocupados y una carga conservadora de bytes insertados/eliminados por transiciones retenidas. La transición anterior al primer estado conservado no se cobra como si todavía fuera undoable. Esto permite deshacer una edición pequeña posterior a un paste grande que ya excedió la cuota.

`Document::set_history_bytes_limit` y `Workbench::set_history_budget` permiten modificar la cuota. Se recortan estados antiguos hasta cumplirla; la geometría necesaria para las transiciones restantes, revisión guardada y ramas de redo conservadas siguen válidas. Presupuestar implica que un cambio enorme puede quedar fuera del historial disponible, incluso si el límite de pasos no se alcanzó.

`reclaim_unused_storage` reúne intervalos de origen todavía alcanzables por documento/undo/redo, compacta esas fuentes y reubica referencias de piezas. **No borra los undos conservados.** Se ejecuta bajo presión de almacenamiento o al configurar límites; no altera bytes, revisiones ni mapas. `compact_storage` permanece como operación distinta y explícita que elimina historial.

La cuota lógica no es un límite duro de RAM privada. Datos vivos, capacidad sobrante de vectores, índice de líneas, caché, buffers temporales y otros documentos deben medirse aparte. La fuente append-only se recolecta cuando rebasa documento vivo + presupuesto de historial + 64 KiB de holgura; el crecimiento de capacidad de Vec puede reservar más que su longitud. Los documentos cerrados siguen reteniendo únicamente los descriptores introducidos en F1.

## Elección de estructura

Se compararon prefijos planos, prefijos por bloques y árbol binario equilibrado de nodos Box sobre las mismas longitudes de piezas y consultas pseudoaleatorias. El ejemplo `range_layouts` verifica que las tres búsquedas producen el mismo índice. La estimación de metadatos excluye overhead del asignador; los incrementos son cambios simulados de longitud, **no transacciones reales de inserción/borrado**. Construcción fuera del cronómetro; cinco lotes de calentamiento y 31 muestras de 1,000 consultas; luego 200 incrementos.

| Piezas | Candidato | 1,000 consultas p50/p95 µs | 200 incrementos µs | Metadatos estimados bytes |
|---:|---|---:|---:|---:|
| 16 | prefix-vector | 4.9/5.2 | 2.0 | 128 |
| 16 | blocks-128 | 4.3/4.4 | 2.1 | 160 |
| 16 | boxed-tree | 4.8/5.4 | 2.4 | 992 |
| 1024 | prefix-vector | 16.6/19.2 | 28.4 | 8192 |
| 1024 | blocks-128 | 10.1/10.5 | 5.8 | 8256 |
| 1024 | boxed-tree | 8.3/10.9 | 9.2 | 65504 |
| 16384 | prefix-vector | 37.9/38.2 | 409.1 | 131072 |
| 16384 | blocks-128 | 17.3/17.4 | 11.2 | 132096 |
| 16384 | boxed-tree | 14.0/14.8 | 24.6 | 1048544 |

Se eligió el índice por bloques: a 16,384 piezas mejora las consultas frente al prefijo plano y usa aproximadamente una octava parte de los metadatos estimados del árbol. El árbol consultó algo más rápido en esa carga; no se concluye que sea peor para toda operación. La producción conserva un único vector de piezas, añadiendo las dos tablas de prefijos del índice por bloques. Una sustitución futura del contenedor requiere medir transacciones y memoria reales; el prototipo de árbol no se incorpora al motor productivo.

Las ediciones aún reconstruyen vectores de piezas; el índice de líneas desplaza su sufijo en O(L). Son costes de metadatos conocidos, no recorridos/decodificaciones de todo el archivo por cada movimiento.

## Medición local reproducible

Equipo: AMD Ryzen 9 8945HS, 8 núcleos/16 procesadores lógicos, Windows 11 Pro 10.0.26300, 16,037,152 KiB de RAM visible. Rust 1.85.1 MSVC; perfil existente `opt-level="s"`, LTO thin, un codegen unit. No se cambió la política de release: comparar perfiles 2/3 sigue siendo trabajo de medición antes de elegir otra política.

```powershell
.\scripts\cargo-local.ps1 run -p jc-core --example range_layouts --release --locked --offline
.\scripts\cargo-local.ps1 run -p jc-app --example f2_text --release --locked --offline
.\scripts\cargo-local.ps1 run -p jc-app --example audit_baseline --release --locked --offline
```

Corpus generado en memoria: `abcdefg\n` y una sola línea de `é😀`; tamaños nominales 4/64 KiB y 1/4/16 MiB (la línea Unicode usa el múltiplo de seis bytes inferior). No escribe fuentes del usuario. Preparación mide insertar el corpus y su primera proyección; no mide apertura fría de disco ni arranque de la aplicación.

Right, posición y 24 filas: cinco llamadas de calentamiento, 31 lotes de 50 llamadas; se ordenan **medias por lote** y se presentan p50/p95 de esas medias, no percentiles de latencia de teclas individuales. Edit+undo: una pareja de calentamiento y 11 parejas individuales, p50 índice 5 y p95 índice 10; cada pareja inserta un carácter y deshace. Se controla el trabajo leído/decodificado, además del reloj. No hubo aislamiento de carga ni medición de RAM privada.

| Bytes | Forma | Preparación ms | Right p50/p95 µs | Posición p50/p95 µs | 24 filas p50/p95 µs | Edit+undo p50/p95 µs |
|---:|---|---:|---:|---:|---:|---:|
| 4,096 | lines | 0.051 | 1.000/1.162 | 0.408/0.436 | 30.550/30.808 | 4/5 |
| 4,092 | long | 0.036 | 7.444/7.818 | 1.356/1.360 | 9.518/9.792 | 9/12 |
| 65,536 | lines | 0.328 | 0.784/0.938 | 0.408/0.416 | 30.432/31.024 | 11/12 |
| 65,532 | long | 0.683 | 6.976/7.616 | 3.482/3.558 | 9.112/9.452 | 18/19 |
| 1,048,576 | lines | 3.760 | 0.826/0.948 | 0.442/0.446 | 30.428/31.530 | 127/132 |
| 1,048,572 | long | 7.892 | 6.864/10.126 | 3.404/3.540 | 8.986/9.160 | 19/20 |
| 4,194,304 | lines | 15.693 | 0.868/0.976 | 0.454/0.536 | 31.150/32.110 | 502/508 |
| 4,194,300 | long | 30.486 | 7.184/7.334 | 3.664/3.730 | 9.318/9.444 | 23/30 |
| 16,777,216 | lines | 56.173 | 0.914/1.340 | 0.452/0.456 | 31.304/32.184 | 1983/2203 |
| 16,777,212 | long | 125.130 | 13.192/14.190 | 6.596/6.790 | 15.514/21.280 | 45/53 |

En todos los lotes calientes el contador de bytes del escaneo global de líneas permaneció en **cero**. Sí se decodificaron fragmentos: `hot_decoded` suma todo el trabajo de las miles de llamadas del experimento; no significa cero lecturas ni cero asignaciones. Los datos completos incluyen tamaños retenidos de proyección/fuentes y cargo de historial.

La [ruta previa a migrar texto](benchmarks/F1-before-F2.csv), usando `audit_baseline` con 5 llamadas de calentamiento y 31 muestras individuales, daba 531.180/561.927 ms de Right p50/p95 en 4 MiB y 39.424/41.477 ms de posición. Esa corrida ya tenía el prototipo de lectura de rangos en core, todavía sin uso por dichas llamadas: caracteriza la ruta textual F1, no certifica un checkout prístino distinto. El [mismo diagnóstico después de F2](benchmarks/F2-audit.txt) resuelve esos movimientos alrededor de la resolución de microsegundos del informe. Los nanosegundos de la tabla anterior vienen de otro muestreo; no se calcula un factor exacto de mejora entre métodos distintos.

En el diagnóstico sin historial, 100 reemplazos de 64 KiB retienen ahora **131,072 bytes añadidos**, frente a los 6,553,600 de la base. Esto mide almacenamiento añadido alcanzado al terminar esa carga, no el consumo completo del proceso.

Datos: [modelo](benchmarks/F2-model.csv), [candidatos](benchmarks/F2-layouts.csv), [diagnóstico](benchmarks/F2-audit.txt). Los objetivos <50 MiB, <0.5 s de arranque y <16 ms tecla-a-pantalla **no se declaran alcanzados** por estas mediciones. Las consultas calientes del modelo sí están por debajo de la meta exploratoria de 2 ms en las cargas ensayadas.

## Validación y límites

**142 pruebas locales satisfactorias** en todo el workspace; Clippy sin warnings, formato y release build. Evidencia en [VALIDACION.md](VALIDACION.md). Hay regresiones nuevas para segmentos contra slices planos, snapshots prestados, recuperación con undo/redo/revisión guardada, cuotas por bytes, transiciones grandes descartadas, fragmentos BOM/offsets absolutos y render visible. La proyección se compara con el decoder/índice completos de referencia tras ediciones arbitrarias por bytes, lotes, UTF-8 inválido y undo/redo en los cuatro encodings soportados.

Límites materiales:

- La primera construcción de un layout frío todavía materializa temporalmente esa línea para crear checkpoints; afecta al pico de memoria y a la latencia fría de una línea extrema. No se retiene ese texto como caché. Dividir/fusionar líneas gigantes puede requerir reconstruir layouts de las líneas afectadas.
- Índices de líneas O(L), vectores de piezas y cuotas por documento no constituyen un presupuesto global de proceso. Metadatos del sufijo, muchas piezas/documentos y presión de recolección requieren más cargas de aceptación.
- Las muestras no incluyen render completo/terminal, IME, grafemas, DPI, GUI, Linux, búsqueda de proyecto, workers ni AVM. No se usó CI remoto ni se tocaron GPU, drivers o servicios ajenos.
- Sigue pendiente restaurar exactamente selecciones históricas: se conserva la política de mapeo determinista de F1.

El siguiente incremento funcional es F3 (presentación de solo lectura y tareas versionadas/acotadas). Mantener estos benchmarks para detectar regresiones y continuar perfilando picos fríos/memoria antes de cerrar objetivos globales. Sublime Text sigue siendo la referencia visual; esta optimización es del motor compartido.
