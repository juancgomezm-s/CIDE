# jc-core

Núcleo inicial de documentos de JC-IDE, sin dependencias externas.

## Contrato

- Almacena bytes arbitrarios. No presupone UTF-8 ni una página de códigos concreta.
- `Range<usize>` expresa offsets **en bytes**, con límite final exclusivo.
- `replace`, `insert` y `delete` son operaciones individuales de deshacer.
- Los rangos inválidos no cambian contenido, revisión ni historial.
- `write_to` conserva los bytes; no convierte codificación, CRLF/LF, BOM o salto final.
- El llamador marca `mark_saved()` solo después de completar su guardado. El núcleo
  no implementa reemplazo atómico de archivos, detección de cambios externos o
  sincronización a disco.
- `Region` conserva la orientación de la selección. Su prueba `contains` incluye
  ambas posiciones de extremo, como Lime Text; `byte_range()` es semiabierto.

```rust
use jc_core::Document;

let original = vec![b'N', 0xf1, b'\r', b'\n'];
let mut document = Document::from_bytes(original.clone());
document.insert(2, b"!").unwrap();
assert_eq!(document.to_bytes(), vec![b'N', 0xf1, b'!', b'\r', b'\n']);
assert!(document.undo());
assert_eq!(document.to_bytes(), original);
```

## Diseño y límites

El archivo original permanece inmutable. Las inserciones se agregan a otro
buffer; el documento es un vector de fragmentos que referencian ambos. El historial
retiene esos metadatos, no una copia completa del texto por pulsación. Se fusionan
fragmentos contiguos del mismo buffer cuando es posible.

Esta primera versión recorre un vector: editar/localizar posiciones cuesta O(P),
donde P es el número de fragmentos. El historial y el buffer de inserciones no
tienen límite ni compactación todavía. No se han establecido cifras de memoria,
latencia ni tamaño máximo admisible; hay que medirlas con fuentes representativos.

`is_modified` compara la identidad de la revisión con el último punto guardado.
Deshacer hasta ese punto limpia el estado; reconstruir los mismos bytes con
ediciones nuevas puede mantenerlo marcado como modificado.

Pendientes: transacciones de múltiples reemplazos, límites de historial,
índice de líneas/árbol de fragmentos según mediciones, transformación de
selecciones tras ediciones, codecs estrictos y mapas de coordenadas visuales.
La interfaz debe impedir partir caracteres multibyte; este almacenamiento permite
cualquier límite de bytes de forma deliberada.

## Procedencia

`Document` es una implementación nueva para este proyecto. `Region` y sus casos
seleccionados de compatibilidad son un port parcial y atribuido de
[`limetext/text`](https://github.com/limetext/text/tree/a501ee418129fedf3fdca3b0d116231dd14af26c),
con licencia BSD de dos cláusulas conservada en
[`third_party/limetext-text-LICENSE`](../../third_party/limetext-text-LICENSE).
Se portan `Begin`, `End`, `Contains`, `Covers`, `Empty`, `Size`, `Cover`,
`Intersection` e `Intersects`. Se cambian las coordenadas a bytes unsigned.
`Clip`, `Cut` y `Adjust` no se trasladan en esta primera entrega: necesitan un
contrato de edición y afinidad de cursor explícito antes de incorporarse.
