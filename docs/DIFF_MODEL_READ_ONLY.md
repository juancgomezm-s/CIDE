# DiffModel — preparación de comparación de solo lectura

Frente independiente del bloque de aceptación M2A, preparado el 2026-11-02.
Propiedad de este worker: `crates/jc-app/src/diff_model.rs` y este documento.
El integrador conserva exports/manifests, comandos, frontend, PLAN, VALIDACION,
compilaciones y commits. No se ha ejecutado Git ni modificado una fuente mediante
este modelo. Es código nuevo de JC-IDE, sin port ni dependencia adicional.

## Contrato

`DiffModel::compute(left: &[u8], right: &[u8], limits, cancelled)` presta dos fuentes
inmutables. No recibe `Document`, `FileSession`, vistas, historial ni buffers
editables. Rust impide modificar los slices mientras el resultado los presta;
su propietario puede ser un snapshot acotado independiente. No copia los bytes.
La captura de documentos vivos, revisión/encoding/generación y entrega F3 son
responsabilidad del coordinador futuro, todavía sin implementar aquí.

El modelo produce:

- `DiffStatus::Exact`, `Limited(InputBytes/Lines/Work/Metadata/Hunks)` o `Cancelled`;
- spans ordenados `Equal`/`Changed`, con rangos half-open de `DiffLine` cero-based;
- hunks como spans cambiados, sin duplicar otra lista; contexto en los spans iguales;
- rangos originales de bytes y líneas prestadas, sin decodificación;
- correspondencia por búsqueda binaria **solo para líneas iguales**. Una modificación
  de una línea por otra no afirma equivalencia ni una posición de caret comparable;
- estadísticas de trabajo, bytes comparados y capacidad lógica máxima de metadatos.

Una limitación/cancelación devuelve un resultado sin spans, índices parciales ni
correspondencias. `line_count`/`line_byte_range` devuelven `None`; se pueden consultar
status/stats y los slices originales. El consumidor debe distinguirlo de un diff
vacío exacto. No hay fallback silencioso que marque el resto como igual/cambiado.

`DiffLine` es una coordenada distinta de bytes, escalares, grafemas o columnas. Una
fuente vacía tiene cero líneas **de diff**. El terminador pertenece a su línea y un
newline final no crea una fila vacía artificial. Por tanto `a`, `a\n` y `a\r\n`
son líneas diferentes. CRLF es un terminador completo; también se admiten CR y LF
mezclados. BOM, bytes inválidos UTF-8 y legacy permanecen literales. No se asume
encoding ni se normalizan EOL o sustituyen caracteres para comparar.

## Algoritmo y cuotas

Myers por líneas exactas, con frontier compacto por distancia de edición y trace
para reconstruir una alineación mínima determinista. No hay tabla LCS N×M en
producción. En el peor caso Myers aún requiere mucho trabajo y el trace crece
cuadráticamente con la distancia: **se corta explícitamente por presupuesto**.
La tabla LCS pequeña aparece solo como oráculo independiente en tests.

Cuotas predeterminadas, combinando ambas fuentes:

| Recurso | Límite |
|---|---:|
| Bytes de entrada | 16 MiB |
| Líneas de entrada | 200.000 |
| Trabajo contabilizado | 64 Mi unidades |
| Capacidad lógica de índices, trace y spans | 16 MiB |
| Hunks | 4096 |

El presupuesto de trabajo cobra bytes indexados, pasos de frontier/backtrace y
bytes de comparación. Cada comparación de línea se fragmenta en hasta 1024 bytes,
comprobando cancelación y presupuesto antes del siguiente fragmento. No existe
un `left == right` global que evite esos controles en una línea extrema. El indexado
comprueba cancelación en bloques de hasta 1024 bytes (más el LF que completa un
CRLF en su borde); también se comprueba por capa/paso al construir y reconstruir
el trace. La callback debe ser barata y no bloquear.

Los vectores crecen geométricamente con reservas fallibles/cobradas, evitando
reallocations lineales repetidas. Las cuotas son de capacidad lógica solicitada,
no de RAM privada: el allocator puede conservar capacidad y realojar durante una
reserva. Una llamada a allocator o callback no es preemptible. No se certifica una
latencia dura; usar el cálculo en una tarea fuera del camino de entrada/render y
proporcionar cancelación/deadline desde su coordinador. Este módulo no crea threads.

## Integración y validación

No añade un comando o renderer de diff, carga de index/HEAD, procesos Git ni
stage/commit/merge. Preparar este modelo no acredita Git operativo ni cierra M2A.
El modelo prestado podrá servir a TUI/GUI sin generar un segundo editor; la futura
fuente Git debe ser un snapshot inmutable separado del documento de trabajo.

El integrador declaró `pub mod diff_model` desde `jc-app/src/lib.rs`. No cambió
Cargo.toml/Cargo.lock ni otra API existente. Validación local offline completada:

```console
cargo test -p jc-app diff_model --locked --offline
cargo clippy -p jc-app --all-targets --locked --offline -- -D warnings
```

Seis tests observables: insertar/borrar/cambiar y correspondencia; EOL/BOM/bytes
inválidos/newline final/vacío; repetición y hunks separados; todos los corpus de
dos letras hasta cuatro líneas contra LCS independiente; cuotas/cancelación sin
resultados parciales; líneas largas con trabajo limitado y cambios adversos con
trace limitado. La suite completa del integrador pasó con **200 pruebas, 0 fallos**;
fmt, Clippy workspace/all-targets -D warnings y release aprobados. Clippy detectó
un bucle indexado en el oráculo de test; tras convertirlo a enumerate se repitieron
las seis pruebas DiffModel y Clippy, sin repetir las suites inalteradas. El worker
no ejecutó Cargo ni benchmarks; las compilaciones se serializaron por el integrador.
No se ha medido throughput/RAM de diff ni existe integración de UI/Git.

Pendientes: medición de cargas representativas, claves de fuente/ruta/SHA y entrega
por revisión, UI unificada/lado a lado, navegación y scroll por hunks, accesibilidad
y proveedor Git de solo lectura. Escrituras Git y merge permanecen en M5.
