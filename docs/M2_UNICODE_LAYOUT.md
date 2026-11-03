# Layout Unicode compartido — cierre local 2026-11-03

Implementado sobre `6b6d790`, después de la autorización explícita de Juan.
La matriz de [pendientes TUI](M2_TUI_FINISH.md) distingue ahora construcción
funcional concluida de aceptación física/global abierta. Sublime Text sigue como
referencia visual; no se declara paridad con su editor o sus plugins.

## Dependencias y coordenadas

`jc-text` fija unicode-segmentation=1.13.3 y unicode-width=0.2.2, esta última sin
features predeterminadas. Tablas Unicode17; segmentación declara MSRV1.85 y width
1.66. Los manifests del workspace declaran1.85, con el Rust1.85.1 ya disponible;
sin instalar programas. Beneficio: UAX29 mantenido y ancho de grafemas sin tablas
caseras. Coste: tablas/binario, avisos y nuevo mínimo declarado.
[Segmentación](https://docs.rs/crate/unicode-segmentation/1.13.3),
[reglas de ancho](https://docs.rs/unicode-width/0.2.2/unicode_width/),
[manifest width](https://docs.rs/crate/unicode-width/0.2.2/source/Cargo.toml).

El documento conserva bytes arbitrarios. TextPosition/Go to y resultados de
búsqueda siguen en sus coordenadas explícitas; no se redondean propuestas externas.
El teclado horizontal y Delete/Backspace sin selección operan sobre grafemas
completos; CRLF es indivisible. Un caret colocado por API escalar dentro de un
grafema se presenta al inicio de éste; borrar por teclado retira el grafema que
lo contiene. Edición de rangos explícitos conserva sus límites de bytes.
Movimiento vertical/reveal/hit testing usan columnas visuales, TAB4 y conversión
a fuente. Una celda interior de un glifo ancho apunta a su inicio.

## Lecturas, cachés y presentación

SourceGraphemes adapta GraphemeCursor a rangos prestados de Document: fragmentos
de512 bytes, contexto UTF8 estricto y solape regional explícito. Conserva grupos
de UTF8 inválido y BOM literal intermedio. Un separador sintético existe sólo en
la entrada temporal del segmentador; nunca se publica como byte de fuente.
Checkpoints dispersos retienen estado regional, offsets y columnas, revisión y
encoding. Ediciones invalidan el sufijo afectado y conservan el prefijo seguro;
no se reconstruye un índice/decodificación del documento para cada movimiento.
La primera vista fría construye sólo su prefijo requerido. Saltar muy lejos en
una línea fría todavía puede recorrer ese prefijo, con límite de trabajo explícito.

Por documento:512KiB layouts escalares +256KiB Unicode +128KiB invisibles
+128KiB sintaxis =1MiB lógico. Unicode conserva hasta64 líneas y contabiliza
capacidades; metadatos estructurales de líneas se miden aparte. No garantía de
RAM privada. Un cursor admite16MiB de lecturas de trabajo, incluyendo contexto
y fallos; fuente excesiva produce error visible, sin cambiar bytes.

VisualAtom contiene rango fuente, columna, ancho, Glyph y marca diagnóstica.
ASCII/escalar simple va inline; sólo clusters usan Arc de texto. El ancho narrow
se calcula por grafema; no implica bidi, ligaduras entre grafemas, shaping de una
fuente ni anchura idéntica en todo terminal. Invalid UTF8/controles/huérfanos de
ancho cero tienen representación ASCII explícita. Grafemas >1024 bytes se muestran
como diagnóstico acotado; su navegación sigue la frontera completa bajo cuota.
El clipping nunca emite medio cluster; una intersección lateral se rellena.

Filas:64KiB de texto de glifos. Presentación fuente de ambas vistas:2MiB en total;
tras agotar presupuesto aparecen filas explícitas de layout no disponible.
FrameLayout limita131072 celdas y dimensiones absurdas usan fallback80×10.
Screen conserva Cell de8 bytes y mapa disperso sólo para clusters/glifos anchos;
no hay String por celda ASCII. FrameEncoder compara payload/estilo, emite átomos
completos, omite continuaciones y limpia toda cobertura previa al reemplazar.
Composición sigue de solo lectura y un frame sin cambios no emite VT.

La GUI consume los mismos átomos y convierte el cluster completo a UTF16 para GDI;
caret/hits/clip comparten geometría. No acredita calidad física de shaping, DPI
o IME. Campos de formularios conservan su presentación ASCII escapada; selección,
movimiento y borrado del texto del campo respetan grafemas completos.

## Rectángulos

Se retira la whitelist de una celda. Se admiten combinantes asociados a base,
CJK, emoji/ZWJ/banderas con ancho narrow por grafema. Se rechazan controles,
decodificación inválida, grafemas de ancho cero y clusters >1024 bytes antes de
activar o editar. La admisión valida la línea afectada:64KiB/4096 celdas; siguen
1024 filas y presupuestos de fuente/clipboard/propuesta de2MiB existentes.

Copy conserva un glifo completo si está totalmente seleccionado. Una selección
parcial de glifo ancho copia los espacios cubiertos; editar consume el glifo
completo y rellena con espacios las celdas no seleccionadas, igual que TAB.
Nunca se parten bytes UTF8 ni clusters. Se resegmenta el resultado acotado de las
uniones reales para validar ancho/cuota, incluyendo ZWJ. Encoding/BOM/EOL siguen
estrictos y cada acción es una transacción F1 con undo único compartido por vistas.
Clipboard rectangular continúa interno tipado; no formato OS inventado.
El render rectangular usa el caché por rangos común, sin reconstruir LineLayout
completo en cada frame; LineLayout queda en admisión/edición acotadas. El antiguo
adaptador de tuplas de una celda rechaza clusters en vez de truncarlos y ya no es
la ruta de presentación de frontends.

## Validación y medición

18 casos Rust nuevos: seis cursor/layout, cinco aplicación/campos, cinco TUI y
dos Screen/encoder. Regresiones del workspace ejecutadas por suites y cierres
dirigidos, no una única corrida global:530 nombres de pruebas aprobadas, dos
helpers Git omitidos como tests independientes e invocados por sus padres,
ningún nombre listado sin cubrir. [Inventario](evidence/m2-unicode-layout-2026-11-03-coverage.json).
Formato, Clippy workspace/all-targets -D warnings, release bins/examples y doctests
aprobaron offline. Fixtures Git/junction propios también aprobaron con acceso
local autorizado; no GitHub Actions.

ConPTY final: cuatro flujos (caret UAX29/CRLF, borrado familia+undo, rectángulo
parcial+undo, resize),45965 bytes VT, salida0 y fixture intacto. Se afirma posición VT y bytes,
no apariencia física de emoji. GUI:17 checks HWND propios, dos nuevos de Unicode,
GDI sin error y fuente/metadata Git intactas. El test de plataforma verifica que
el propietario clipboard responda WM_NULL inactivo sin abrir el clipboard OS.

Ejemplo reproducible `jc-app --example unicode_layout --release`: corpus sintético,
vista de80 celdas, una llamada fría y101 calientes, p50/p95 individuales; sin
aislamiento de carga. [CSV final](evidence/m2-unicode-layout-2026-11-03-measurement.csv)
incluye alloc/heap, RAM del proceso modelo y trabajo leído. Scalar_reference es
la API escalar F2 del mismo binario, no un checkout anterior y no semántica idéntica
en Unicode. La última columna mide insertar al principio y preparar fila, pese
al nombre histórico suffix_edit_row_ns de las variantes iniciales.

Se compararon fragmentos4096/512;512 redujo lecturas ASCII de207252 a26260 bytes
por101 vistas calientes. [4096](evidence/m2-unicode-layout-2026-11-03-window4096.csv),
[512 antes de reserva](evidence/m2-unicode-layout-2026-11-03-window512-before-reserve.csv).
No se calcula mejora global de RAM/latencia a partir de esas llamadas. El intento
ConPTY comparativo de latencia no logró confirmar el primer movimiento ASCII y
no aporta muestras aceptadas; se conserva el reporte fallido. Sólo tamaños/hashes
de ejecutables se comparan contra la [base6b6d790](evidence/m2-unicode-layout-2026-11-03-baseline.json).
Los [metadatos finales](evidence/m2-unicode-layout-2026-11-03-metadata.json) registran
delta de binarios, fuente, pruebas, recorridos y paquete CRC/manifiesto verificado.

Fallos conservados: contexto de banderas al cruzar fragmentos (corregido con
solape), writer de áreas Git que avanzaba por char (migrado a clusters), fixtures
de repertorio antiguo, colisión de IDs nativos regex/Git (regex165), y lints.
Un helper Python leído sin UTF8 alteró literales de código/fixtures en dos archivos;
se repararon con conversión inversa exacta. La regresión F2 detectó el caso y pasó
al corregirlo; no se modificaron documentos del usuario. ConPTY inicialmente envió
BS08 en vez de DEL7F para Backspace; se corrigió el oráculo, conservando fallos.

## Puertas que siguen abiertas

Aceptación física de terminal/fuente, IME, accesibilidad, clipboard entre apps y
latencia/RAM/arranque global. Página legacy real del usuario sin confirmar.
AVM/OpenEdge y semántica M3/M4 requieren entorno y ejecución reales; Git escritura
y CI corresponden a M5, runtime externo post-M5. No shell embebido, multicaret
persistente ni nuevas ampliaciones de ratón admitidas por cerrar este bloque.
Los pendientes funcionales TUI del alcance admitido están implementados con
evidencia local; M2 completo no se declara aceptado.

## Muestras finales y coste de binario

| Corpus | Bytes fuente | Vista fría µs | Caliente p50/p95 µs | Lecturas101 vistas B | Caché Unicode B |
|---|---:|---:|---:|---:|---:|
| ascii-long | 2097152 | 22.9 | 5.3/5.4 | 26260 | 744 |
| unicode-long | 1474560 | 46.4 | 7.4/7.8 | 81305 | 744 |
| ascii-lines | 900000 | 4.6 | 0.7/0.8 | 707 | 744 |

Lecturas incluyen contexto y cruces de fragmentos, no bytes únicos del archivo.
El caché mostrado es el usado por esa vista, no su cuota ni RAM total.

- `cide-tui.exe`: 2411520 bytes, delta +181760 frente a6b6d790.
- `cide-gui.exe`: 2885632 bytes, delta +89088 frente a6b6d790.

Delta incluye el bloque completo, no atribuye todo a tablas Unicode. Binarios
comparados con el mismo perfil release; no mide mejora de RAM de la aplicación.

Paquete local de desarrollo: `work/packages/CIDE-windows-dev-tui-unicode-2026-11-03.zip`,
5741194 bytes/82 miembros, SHA256
`82143e4020744f76f10aec2aec7d6f2c028443f69e087b0f3e20eeefdf9b6510`.
CRC y hashes del manifiesto verificados; dos avisos GNU ausentes declarados,
sin certificación de distribución. [Verificación](evidence/m2-unicode-layout-2026-11-03-package.json).
