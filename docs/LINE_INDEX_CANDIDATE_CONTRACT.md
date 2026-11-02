# Contrato de comparación — índice por bloques, sin integración productiva

Siguiente bloque M2-F después de distribuir frío y medir edición densa nativa.
El índice plano acotado redujo el pico histórico del corpus editado de 68,16 a
52,46 MiB; la meta global sigue abierta. Se comparará un candidato antes de elegir
otra estructura. No se conecta un segundo índice al editor ni se altera el core.

## Propiedad y API experimental

Worker de estructura: solo `crates/jc-app/examples/support/block_line_index.rs`.
Tipo `pub struct BlockLineIndex` Default y `pub struct Entry { pub start:usize,
pub ending:u8 }` Copy/Debug/PartialEq/Eq. Métodos públicos compatibles con los
del LineIndex existente: len, capacity (slots), memory_bytes (capacidad lógica
incluidos headers/catálogo), get->Option<Entry>, start, ending, first_start,
entries->impl ExactSizeIterator<Item=Entry>, upper_bound, push(start,ending),
finish(start), widen_for(max_offset), shift_tail(from,old_len,new_len),
replace_after(Range<usize>,replacement:Self), shrink_to_fit.

Candidato: bloques pequeños con base de byte usize y offsets relativos compactos
más EOL. Un bloque puede partirse al agotar rango relativo; ninguna conversión
trunca usize ni EOL, incluso usize::MAX. Líneas cortas deberían ocupar menos de
cuatro bytes/registro contando catálogo/headers. El desplazamiento de un sufijo
completo actualiza bases por bloque; las fronteras parciales se reconstruyen
localmente. No clonar/reconstruir todos los registros por cada cambio. Evitar
holgura geométrica del catálogo completo por una línea adicional y documentar
costes/patologías. Un solo formato/catálogo por índice experimental.

Worker de comparación: solo `crates/jc-app/examples/line_index_candidates.rs` y
`docs/LINE_INDEX_CANDIDATES.md`. Importa el candidato por support/ y el índice
productivo mediante `#[path = "../src/workbench/line_index.rs"] mod flat;`, sin
copiarlo ni exportar APIs privadas. Tipos se adaptan en el driver; no exigir que
Entry de ambos módulos sea el mismo tipo. Ningún archivo Cargo/lib/productivo.

## Protocolo mínimo

Comparar construcción fría, capacidad retenida, get/upper_bound calientes y
edición/inversa de byte y de frontera de línea en inicio/medio/EOF. Corpus de
1/4/8 MiB con líneas cortas de 8 bytes y líneas de 64 bytes; casos pequeños de
línea extrema y offsets grandes/BOM/mezcla de EOL como regresiones de metadatos.
Mismas cuotas/cargas, calentamientos y 31 muestras por operación. Las consultas
pueden agruparse para medirlas, conservando unidad y tamaño del lote explícitos.

Preparar reemplazos fuera del timer. Para insertar `x\n` al inicio de una línea i:
shift_tail(i+1,N,N+2), después replace_after(i+1..i+1, contexto y nuevo inicio).
La inversa elimina esa frontera y desplaza el resto. Comparar entradas/EOL y
upper_bound contra un vector oráculo independiente y el índice plano, incluyendo
secuencias deterministas de cambios, sin convertirlo en otra ruta productiva.
No publicar capacidad lógica como RAM privada o tiempo de metadatos como UI.

Ambos workers construyen código completo antes de pruebas. No ejecutar Cargo,
fixtures ni mediciones: el integrador revisa, compila y reserva ventana serial.
La selección/integración productiva requiere esos datos y eliminar la ruta
sustituida, manteniendo byte/encoding/mapas/undo y lecturas de rango.
