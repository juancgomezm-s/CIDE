# CPU — cierre de ejecución por archivos

Implementado y medido localmente el 2026-11-02. Search Plus usa dos trabajadores
por defecto, contando su coordinador, con límite configurable 1…4 y admisión
conservadora según procesadores disponibles. El usuario puede arrancar con
`jc-ide-tui --search-workers 1` para limitar demanda, o solicitar hasta cuatro.
La [opción de arranque](CPU_RUNTIME_OPTIONS.md) llega a cada nueva consulta.

## Elección basada en perfil

[Hardware observado](benchmarks/M2-cpu-hardware.json): Ryzen 9 8945HS, ocho núcleos
físicos, 16 lógicos y máscara disponible 0xffff. Consultas de 16 MiB/16 archivos
del [perfil inicial](benchmarks/M2-cpu-profile-summary.csv) consumieron aproximadamente
0,994–1,002 núcleos equivalentes agregados. Medianas de pared 131,878–248,002 ms
según consulta/encoding. Decoder prestado y diff se perfilaron aparte, con sus
checks y límites; no se eligió dividir un archivo ni paralelizar Myers sin evidencia.

Se reemplazó el recorrido monolítico por [un executor de proyecto](CPU_PROJECT_EXECUTOR.md):
coordinador/scanner0 más helpers reutilizables, patrón y Request compartidos,
fuentes abiertas prestadas y orden/cuotas cobrados centralmente. Workers=1 usa
el mismo engine. JobSystem sigue siendo documental y ya dispone de dos workers;
no se fabrican documentos ni se anida un pool dentro de sus proveedores.

La admisión limita a `min(solicitados, clamp(disponibles−4,1,4))`. Esto limita
nuestra demanda; no reserva núcleos físicos en el SO ni garantiza CPU libre para
Qwen. Los otros servicios cuentan aparte. Los helpers esperan sin trabajo entre
consultas y no conservan el Request/snapshot anterior. No hay afinidad, prioridad,
energía, GPU ni procesos ajenos modificados.

## Comparación principal

Binario serial guardado desde `16071e3`, mismo ejemplo y SHA-256, frente al nuevo
executor con default dos. Rust 1.85.1 MSVC, release `s`, thin LTO, codegen-units 1,
locked/offline. Cuatro procesos en orden serial/paralelo y paralelo/serial.
Doce casos por proceso: 1/4/16 MiB, UTF-8/Windows-1252 explícitos, cero/pocas
coincidencias. Un warmup y 31 muestras por caso: **1488 muestras conservadas**.
Los 12 casos mantuvieron tamaños/hash de corpus, filas, checksum, cero issues y
estado Complete. El protocolo verifica también ruta/rango único y cada hit esperado.

16 MiB, milisegundos; valores primer / segundo par:

| Consulta | Mediana serial | Mediana dos workers | p95 serial | p95 dos workers |
|---|---:|---:|---:|---:|
| UTF-8, cero hits | 185,190 / 213,604 | 148,920 / 147,629 | 226,016 / 225,599 | 156,350 / 152,359 |
| UTF-8, pocos hits | 209,222 / 212,512 | 151,467 / 152,173 | 227,458 / 226,436 | 161,112 / 159,520 |
| Windows-1252, cero | 235,659 / 235,685 | 158,042 / 157,943 | 240,994 / 241,294 | 166,837 / 166,174 |
| Windows-1252, pocos | 235,208 / 230,757 | 159,778 / 160,793 | 244,988 / 245,283 | 169,281 / 167,760 |

En esos casos, mediana **1,24–1,49× más rápida**, p95 1,41–1,48×. Throughput
mediano de corpus elegible completo, aproximadamente 67,9–86,4 → 99,5–108,4 MiB/s.
Es el recorrido start/scan/entrega/poll/verificación, no ancho de banda del disco
ni rendimiento aislado del matcher. Son datos de espera de búsqueda, no tecla-a-pantalla.

El uso agregado pasó de 0,993–1,006 a **1,726–1,797 núcleos equivalentes** en las
series de 16 MiB. CPU user+kernel se suma por operación antes de calcular cualquier
cuantil; nunca se suman p50/p95 independientes. La suma de CPU de 31 operaciones
por caso **aumentó 30,4–47,9 %**. Se acepta el intercambio local de mayor CPU total
por menor espera, conservando un límite bajo por defecto. No se declara menos
trabajo de CPU ni speedup lineal por cada núcleo añadido.

En 1/4 MiB, 23 de 24 comparaciones de medianas mejoraron; Windows-1252/pocos/1 MiB
en el primer par fue aproximadamente 4,2 % más lento. Todos los p95 de ese conjunto
mejoraron en la captura. No se oculta esa mediana ni se generaliza la mejora a
un archivo único, todas las consultas o cualquier carga concurrente.

Memoria privada máxima **observada por fase** del harness de 16 MiB:
serial 5,527–5,625 MiB, paralelo 3,520–4,230 MiB. También se retiró el crecimiento
del Vec para leer el byte guardia de EOF; el ahorro no se atribuye a hilos gratis.
Son muestras del proceso headless, no la RAM de toda la TUI ni aceptación global
de 50 MiB. Working set y pico histórico de compromiso/pagefile quedan en columnas
separadas. El sampler puede omitir transitorios breves.

[Resumen por caso](benchmarks/M2-cpu-search-summary.csv),
[ratios por par](benchmarks/M2-cpu-search-comparison.csv),
[raw serial r1](benchmarks/M2-cpu-search-serial-r1-samples.csv),
[raw paralelo r1](benchmarks/M2-cpu-search-parallel-r1-samples.csv).
Los cuatro prefijos `M2-cpu-search-{serial,parallel}-r{1,2}` incluyen raw, checks
y metadata con hashes de binario, probe y fuentes runtime.

## Decisión de compilación

Se compararon overrides **solo del paquete jc-app** `s/2/3`, sin modificar el
manifest ni los demás crates. Mismo executor dos workers y probe/corpus de 16 MiB:
dos órdenes inversos, siete muestras/caso, seis capturas y 168 muestras conservadas.
Con siete, p95 es el máximo; es selección preliminar, no estimación estable de cola.

El binario del probe pasó de 498176 B a 576000/607232 B (+15,6/+21,9 %).
`2` empeoró varios casos UTF-8; `3` tuvo ganancias pequeñas y signo variable en
UTF-8 cero, además de mejora local mayor en legacy. No hay evidencia suficiente
para cambiar la política global de release por ese aumento de código: **se mantiene s**.
No se presenta el tamaño de ese probe como tamaño de la TUI ni como RAM privada.
[Selección de codegen](benchmarks/M2-cpu-codegen-summary.csv); se conservan los
seis prefijos crudos `M2-cpu-codegen-{s,2,3}-r{1,2}` y sus flags/hashes.

## Integración y validación

El bloque CPU reunió **274 pruebas distintas aprobadas** al cierre; no fue una
única segunda suite completa de 274. La corrida workspace aprobó 122 casos de
aplicación antes de detenerse en dos oráculos nuevos que confundían S1 sensible
a mayúsculas con S0 full folding. Se corrigieron solo esas expectativas; el módulo
nuevo y las suites de aplicación pendientes aprobaron 32 casos, y los demás crates
115: 269 distintos. Se revisó después la frontera exacta con archivo vacío mediante
el caso dirigido correspondiente, sin repetir suites ya aprobadas.

La opción TUI se construyó después: sus 47 pruebas (42 biblioteca y cinco parser)
pasaron, incluyendo cinco nuevas; total distinto **274**. Comprueban defaults,
1…4, encoding/rutas, errores/duplicados, validación antes de entrar en terminal y
propagación a cada draft de consulta. Release CLI real verificó help y errores
sin abrir terminal. Formato, Clippy workspace/all-targets del executor, controles
dirigidos posteriores de TUI/test afectado y release pasaron. El ajuste manual_clamp
pedido por Clippy fue equivalente; no motivó otra suite. Sin CI remoto.

Pruebas preservan orden/rangos/posiciones/encodings, cuotas globales, abiertos
sucios/revisión/undo, cancelación/supersesión con backlog y cierre sin ack. Internas
comprueban liberación del último Arc antes de idle, reutilización y unwind del
coordinador con sender activo. No se paralelizan escrituras de un documento.

## Límites y reproducción

La carga de **Qwen es desconocida** y así figura en todas las capturas. No se
enumeraron/controlaron sus procesos ni se consultaron archivos privados.
Cachés del SO/CPU, energía, temperatura y otra carga concurrente no están
controladas. La variación entre el perfil inicial y los pares posteriores no
se atribuye toda al código; la comparación usa pares con el mismo protocolo.
Windows CPU accounting está cuantizado: fases cortas pueden dar cero o más de
100 % de un núcleo; los agregados largos son la evidencia de utilización.

El [método](CPU_WORKLOAD_METHOD.md) detalla temporizadores, sampler, preparación
y checks. `scripts/summarize-cpu-evidence.py` valida identidades/cobertura y genera
resúmenes desde raw; `--codegen-only` genera la selección de flags. Requiere destinos
ausentes, no sobrescribe evidencia. `scripts/cpu-hardware.py` registra topología
solo lectura. El probe conserva --samples/--suite/--size-mib y carga etiquetada.

Windows 11 es la plataforma medida; no se declara Linux, GUI ni latencia física.
Workers=4 tiene cobertura funcional, **no comparación de rendimiento aceptada**.
I/O bloqueante puede prolongar cancelación/join. Las cuotas y el orden no convierten
la búsqueda en snapshot atómico del filesystem. La página legacy real del usuario
sigue sin confirmar. La presentación TUI de estado Git permanece como siguiente
incremento funcional autorizado; no se prolongan optimizaciones sin otra pregunta.
