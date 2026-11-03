# Admisión autorizada — layout Unicode general de TUI

2026-11-03. Admisión histórica, posteriormente autorizada e implementada.
El contrato y la evidencia vigentes están en [M2_UNICODE_LAYOUT](M2_UNICODE_LAYOUT.md).
La propuesta siguiente conserva su contexto; no describe pendientes actuales ni
certifica aceptación física/global.

## Dependencias concretas propuestas

Fijar unicode-segmentation=1.13.3 para UAX29/grafemas y unicode-width=0.2.2 con
default-features=false para anchos. Solo proyecto/cache Cargo, no programas del
sistema. Segmentación declara MSRV1.85 y datos Unicode17; width declara MSRV1.66,
sin runtime externo. El toolchain fijado1.85.1 ya satisface ambas; corregir metadata
rust-version de los crates consumidores/dependientes al mínimo real1.85 sin
instalar otro compilador. Verificar manifests efectivos antes de fijar lock.

Beneficio: reglas mantenidas/ensayadas y separación entre bytes/escalares/grafemas/
celdas. Costo: tablas estáticas/binario, dos avisos de licencia y nuevo mínimo
declarado1.85; medir delta de release y memoria de layouts antes de aceptación.
Los tamaños de fuente/rustdoc no son el costo de binario. No implementar tablas
caseras para eludir la restricción de dependencias vigente.

Fuentes primarias consultadas: [segmentación1.13.3](https://docs.rs/crate/unicode-segmentation/1.13.3),
[anchos0.2.2/reglas](https://docs.rs/unicode-width/0.2.2/unicode_width/),
[manifest de width](https://docs.rs/crate/unicode-width/0.2.2/source/Cargo.toml).
Width modela columnas, no shaping bidi, fuente, IME o una garantía idéntica en
todos los terminales. Sus ligaduras de algunos scripts pueden cruzar grafemas:
no basta sumar anchos independientes sin revisar esa regla.

## Bloque propuesto después de autorización

1. API de layout por línea/rango revision+encoding, tipos explícitos para
   GraphemeBoundary y DisplayColumn; BOM inicial fuera del contenido y CRLF
   indivisible. Invalid UTF8 conserva byte original y representación visible.
   Caches F2 siguen1MiB/documento; nunca decodificar documento completo para mover.
2. Una geometría compartida para caret, selección, rectángulo, TAB4 y clipboard;
   no dos motores de edición. Congelar la política narrow para ancho ambiguo,
   clipping por átomo y selección rectangular que cruza un glifo de dos celdas
   antes de habilitarlo. Fuentes sin interpretación siguen byte-preserved.
3. Screen/FrameEncoder deben emitir un cluster completo, distinguir celdas de
   continuación, comparar payload además de estilo y limpiar el ancho anterior.
   Fast path ASCII sin String por celda. Arena sparse/filas acotadas a evaluar
   frente a representación actual; medir tamaño/alloc/cold antes de elegir.
4. Migrar movimiento/render/campos y adaptaciones GUI afectadas por coordenadas
   compartidas, retirar la ruta sustituida. Las APIs de edición por bytes y
   resultados de búsqueda mantienen sus rangos; no redondear propuestas externas.
5. Construir el conjunto antes de validar: combinantes, CJK, emoji/ZWJ/flags,
   TAB/CRLF/BOM/legacy/bytes inválidos, cortes laterales y estilos, rectángulos y
   undo/redo, dos vistas y delivery obsoleto. Unit/event+ConPTY y medidas locales;
   pantalla real/terminal/fuente/IME siguen requiriendo aceptación física.

En la admisión aún no se habían añadido los crates. Ahora ambos están fijados.
La autorización anterior de Juan cubrió regex-automata; [CONTINUAR_EN_EQUIPO](CONTINUAR_EN_EQUIPO.md) conserva la
restricción histórica «sin instalar dependencias». Juan autorizó ambas bibliotecas el2026-11-03 mediante respuesta explícita.
No pedir de nuevo autorización para ellas. CI/AVM siguen fuera del bloque.
