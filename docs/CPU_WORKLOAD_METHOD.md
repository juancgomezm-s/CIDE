# Protocolo CPU opt-in antes de paralelizar

**Ejecutado por el integrador:** perfil inicial, dos pares de comparación y selección de codegen completos, sin repetirlos tras la opción TUI. Resultados y decisiones en [CPU_PERFORMANCE](CPU_PERFORMANCE.md).

`crates/jc-app/examples/cpu_workloads.rs` construye `cpu-workloads-v1`. No cambia
APIs, dependencias, pool de tareas, afinidad, prioridad o paralelismo productivo.
El trabajador preparó código y método sin ejecutar Cargo, fixtures o mediciones.
El integrador debe compilar y ejecutar en una ventana serial antes de atribuir
un cuello de CPU o elegir una optimización.

## Ejecución y repetibilidad

Comandos previstos, no evidencia de ejecución:

```powershell
cargo build -p jc-app --example cpu_workloads --release --locked --offline
.\target\release\examples\cpu_workloads.exe --suite search --size-mib 16 --samples 7 --load-label unknown_qwen_unknown
.\target\release\examples\cpu_workloads.exe --suite decoder --samples 7 --load-label unknown_qwen_unknown
.\target\release\examples\cpu_workloads.exe --suite diff --samples 7 --load-label unknown_qwen_unknown
```

CLI: `--suite search|decoder|diff|all` (all por defecto), `--samples 1..31`
(7 por defecto), `--size-mib 1|4|16` y `--load-label` ASCII acotado. El selector de
tamaño filtra búsqueda/decoder; diff usa siempre 4 MiB por lado. Decoder no tiene
un caso de 16 MiB. Un calentamiento por caso se emite como `warmup`, separado de
las muestras. La comparación posterior usa 31 muestras con idéntico código del
probe, corpus, cuotas, batch, orden y opciones; cambiar 7 a 31 amplía distribución,
no constituye por sí mismo una comparación válida de antes/después.

CSV en stdout; contexto, hashes y preparación en stderr. Guardar ambos sin
mezclarlos, junto con hash SHA-256 del source del probe y binario, revisión Git,
toolchain, flags/perfil release y línea de comando. Cada fila incorpora versión
del protocolo y hash FNV-1a64 del corpus. FNV identifica el fixture determinista;
no es una comprobación criptográfica de procedencia. El checksum de resultado
permite detectar cambios de semántica entre corridas.

El integrador registra hardware, energía/temperatura y contexto de ejecución.
El equipo comunicado para esta ronda es Ryzen 9 8945HS, 8 cores/16 lógicos y
afinidad 0xffff; el probe no lo verifica ni cambia. La carga de Qwen u otros
procesos concurrentes se considera desconocida hasta observación externa: el
label predeterminado conserva esa incertidumbre. No enumera ni controla procesos
ajenos. No vacía cachés, cambia energía o ejecuta trabajo concurrente adrede.

## Corpus y escenas

Los archivos se crean con `create_new` bajo una carpeta exclusiva
`work/test-temp/cpu-workloads-PID-TIMESTAMP` de este workspace. Se escriben por
bloques de 64 KiB. Una línea ocupa 256 bytes, termina en CRLF y contiene texto
multibyte UTF-8 o bytes Windows-1252 explícitos. Hay una marca ASCII de 10 bytes
por bloque, sin NUL. Windows-1252 es un corpus sintético, no una afirmación sobre
la página de códigos real del usuario. Preparación y hash se excluyen del timer
de las cargas y su tiempo de pared se registra por separado.

Se valida la ubicación resuelta de work/base/root y se rechazan reparse points
en las rutas propias. La limpieza vuelve a verificar la raíz dentro del workspace
y retira solo archivos creados por este probe y sus directorios exactos, sin
borrado recursivo. Un fallo de limpieza se informa conservando la ruta; no se
intenta ampliar permisos ni borrar fuera del árbol propio.

| Carga | Corpus/protocolo fijo | Verificación y cota |
|---|---|---|
| ProjectSearch, zero | 4 × 256 KiB = 1 MiB; 16 × 256 KiB = 4 MiB; 16 × 1 MiB = 16 MiB, ambos encodings. Query ausente. | Complete, cero matches/issues. Archivos bajo el máximo público de 2 MiB por archivo. |
| ProjectSearch, few | Mismos archivos, query ASCII marcada. | Complete, exactamente 16/64/256 matches, encoding esperado y cero issues; no alcanza la cuota de 1000 resultados. |
| Decoder prestado | Corpus de 1/4 MiB concatenado una vez fuera de muestras; ambos encodings; batch 8. | `DecodedScalars` sin String ni vector de mapas por iteración; rangos contiguos, valid=true, consumo completo y checksum de scalar+byte final. |
| Diff sparse | 4 MiB UTF-8 por lado, líneas de 256 B, un cambio cada 1024 líneas; batch 4. | `DiffModel::compute` prestado, límites por defecto, Exact obligatorio, checksum de spans/hunks y DiffStats. |
| Diff adversarial | Mismos tamaños, primera byte de todas las líneas distinta; batch 4. | Admite Exact o Limited explícito según cuotas por defecto, nunca Cancelled; informa motivo de límite y trabajo, sin inventar alineación parcial. |

La búsqueda utiliza `ProjectSearch::start/poll`, Workbench vacío y Wakeup reales;
no abre documentos ni captura buffers artificiales. El calentamiento incluye el
arranque lazy del worker; las muestras reutilizan ese worker. Cada start limpia
resultados anteriores dentro de la fase. Se drenan los lotes listos antes de
esperar Wakeup para no añadir una espera artificial al final de la cola. Hay un
plazo de espera del harness de 15 s; un resultado Limited/Failed/Cancelled en las
escenas de búsqueda esperadas Complete falla el protocolo, en vez de presentar
ese tiempo como escaneo completo.

ProjectSearch no expone contador público de bytes examinados. El throughput se
etiqueta `complete_eligible_corpus`: bytes elegibles de estos fixtures conocidos
divididos por tiempo final de la operación completa verificada, incluyendo I/O,
fingerprint, matching, snippets, transporte de lotes y poll. No es un contador de
bytes realmente leídos, ancho de banda de disco ni rendimiento exclusivo del
matcher. En few incluye producción/entrega de snippets. No se añadió un caso
artificial dominado por el límite de hits.

Decoder cuenta scalars y valida sus rangos de bytes, sin confundirlos con columnas
visuales o grafemas. Su checksum tiene coste deliberado y fijo: la fase mide
iterador más comprobación, no instrucciones puras del decoder. Diff no decodifica
las entradas y usa 32 768 líneas combinadas, bajo las 200 000 permitidas. Se
prepara una copia derecha acotada fuera de muestras, nunca una copia por
iteración. Un resultado Limited no recibe throughput; `work_units`,
`compared_bytes` y `metadata_peak_logical` proceden de DiffStats, sin afirmar RAM.
No se fabricó una carga F3 solo para mantener ocupados los dos workers: la
evaluación del pool corresponde a auditoría y una carga útil elegida después.

## CPU, memoria y salida

Cada fase tiene Instant y deltas de `GetProcessTimes`: user/kernel en ns, con la
resolución real del contador Windows. El formato FILETIME de 100 ns no promete
precisión física de 100 ns; una muestra corta puede observar CPU cero por
cuantización. Los corpus de 16 MiB y batches fijos reducen ese riesgo. No se
descartan ceros, colas ni muestras lentas.

Un monitor propio se inicia antes de medir y hace join después. Lee únicamente
el pseudo-handle `GetCurrentProcess`, `GetProcessTimes` y
`GetProcessMemoryInfo`, con intervalo solicitado de 10 ms y sin controlar la
planificación del sistema. Publica máximos observados de PrivateUsage y WorkingSet,
además de máximo CPU por intervalo observado. Inicio/fin también se consideran
en la memoria máxima observada. Se emiten `memory_observations` y
`cpu_interval_observations`: cero intervalos de CPU significa que no hubo ventana
del sampler, aunque la columna de máximo muestre cero. El intervalo efectivo
puede superar 10 ms y omitir picos más breves.

`cpu_pct_one_core = (user+kernel)/wall × 100`; 100 % representa un core lógico de
tiempo de CPU, no el total de los 16 lógicos. Puede superar 100 % cuando trabajan
varios hilos, y el máximo por intervalo puede resultar ruidoso por cuantización.
Incluye el coste del monitor y todos los hilos propios de la fase. No incluye CPU
de Qwen ni identifica cuánto tiempo se perdió por contención externa.

`private_before/after/peak_observed` y `working_before/after/peak_observed` son
contadores Windows de este proceso. `lifetime_peak_working` y
`lifetime_peak_pagefile` son máximos acumulados del proceso, separados de los
máximos observados por fase; PeakPagefileUsage no se renombra como pico privado.
No equivalen a heap lógico, memoria física exclusiva o aceptación global de RAM.
Las fases comparten proceso y el allocator puede retener memoria de fases previas;
para comparar picos de una escena, ejecutar la misma selección en procesos nuevos.
El monitor no instala un allocator instrumentado que altere el coste de cada
asignación. En otras plataformas falla explícitamente, sin rellenar CPU/RAM con
ceros simulados.

Se conservan filas raw y se calculan p50/p95 por columna con nearest-rank,
excluyendo warmup. Con 7 muestras, p95 coincide con el máximo y no estima una cola
estable. Los contadores/checksums del resumen describen la última operación; se
verifican en cada repetición, no son percentiles. El throughput del resumen se
deriva de su cuantil de tiempo de pared (`throughput_from_wall_mib_s`): p95 en esa
fila es la tasa correspondiente a latencia p95, no un percentil 95 de tasas.

Antes de optimizar, revisar primero los raw: CPU user/kernel y pared de search
zero/few por tamaño/encoding; decoder como referencia de interpretación prestada;
diff con sus límites/trabajo explícitos; dispersión y carga desconocida. Una fase
con pared alta y CPU baja no prueba por sí sola un cuello de I/O, ni CPU alta
demuestra que añadir hilos mejore la experiencia. El perfil inicial identifica
dónde investigar; cualquier cambio productivo y su comparación posterior forman
otro bloque, con las mismas garantías de bytes, cuotas y entrega.
