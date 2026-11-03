# Regex real en Search Plus — cierre local 2026-11-03

Implementado sobre61e0616, por autorización explícita de Juan de incorporar una
dependencia Rust mantenida y continuar los pendientes TUI en bloques amplios.
`regex-automata =0.4.18` está fijado en jc-app y Cargo.lock; regex-syntax0.8.11
es el parser transitivo. No instalación de programas del sistema ni CI remoto.

## Uso y semántica

Alt+R en Find/Replace alterna regex; el campo muestra R:0/R:1. Menú/paleta
ofrecen Regular Expression. En la consulta de proyecto Alt+R cambia el mismo
modo; el título distingue REGEX/literal. Al enviar la consulta se captura el
flag en ProjectSearchOptions y cambiarlo después no reinterpreta resultados.
GUI comparte el motor y conecta Alt+R/menú/paleta; se compiló, sin nueva corrida
HWND específica de regex. La prioridad de esta validación es TUI.

Soporta alternativas, grupos no semánticos, clases/rangos, cuantificadores
codiciosos/no codiciosos, propiedades Unicode, flags inline, anclas y patrones
multilínea. Son anclas de fuente, no de fragmento. Capturas pueden agrupar el
patrón, pero **el reemplazo es literal**: `$1` inserta los dos caracteres `$1`.
Se conserva preview/confirmación en proyecto y una transacción por documento.
No se promete atomicidad global de archivos. Buscar selección fuerza literal.
Historial conserva regex, case y whole-word con la consulta.

Regex usa case folding simple Unicode del motor; literal conserva full folding
Unicode17 del editor. No se normalizan NFC/NFD ni EOL. Whole Word filtra límites
del match elegido usando la regla Unicode del editor; no cambia la prioridad de
alternativas del patrón. Un match no puede entrar dentro de CRLF ni de un escalar.
BOM inicial se omite del texto interpretable; BOM interior permanece literal.
Los tres encodings legacy se interpretan explícitamente y los resultados vuelven
a bytes originales. UTF-8 inválido termina esta búsqueda como Unavailable; no se
inventa U+FFFD ni se permite reemplazar resultados incompletos.

Look-around y backreferences no están admitidos. El DFA tampoco admite `\b`
Unicode: devuelve error de capacidad; `(?-u:\b)` solicita frontera ASCII.
Los errores se muestran y dejan el estado Unavailable, sin editar fuente.
Coincidencias vacías se enumeran finitamente en fronteras legales; se omite la
vacía adyacente al final del match no vacío anterior, como el iterador del motor.
F3 repetido avanza entre vacías vigentes. Replace-all permite inserciones vacías
y produce un solo undo; nunca se ejecuta contra una búsqueda limitada/cancelada.

## Propiedad, cuotas y costes

Un Pattern compartido selecciona matcher literal o DFA. Documento y proyecto
usan Scanner; no hay segundo motor de edición. El DFA compilado se comparte por
Arc con los workers. Proyecto compila antes de cancelar/capturar y transmite el
Pattern ya validado, sin recompilar en cada archivo. Documento reutiliza el
Pattern al editar/cambiar revisión; query/flags distintos lo reemplazan.

La ejecución DFA es reanudable, anclada en cada candidato escalar; conserva un
cursor, siguiente candidato, mejor final y estado de autómata. Solo retiene un
fragmento original reutilizable de hasta step_bytes+4; no materializa el texto
decodificado completo ni un mapa por escalar del documento. El contexto anterior
mantiene anclas al atravesar fragmentos. Cada transición UTF8, reintento y EOF
carga el presupuesto de trabajo, incluso si vuelve atrás en la fuente.

Las cuotas SearchLimits existentes gobiernan tamaño del turno, deadline, trabajo
total y resultados. En regex, max_scan_bytes limita **trabajo**, incluyendo
reintentos/UTF8 interpretado; no promete examinar esa cantidad de fuente única.
Un escalar puede exceder la cota del turno/trabajo hasta tres bytes. El reloj se
consulta entre escalares; no es preempción ni garantía dura de2ms.

Compilación: consulta1..4096 UTF8, nesting64, NFA1MiB, DFA1MiB y workspace de
determinización2MiB. Compilar es una llamada acotada por tamaño, no preemptible.
El escaneo anclado puede tener trabajo cuadrático con candidatos largos que
fallan; el límite/deadline lo termina explícitamente como Limited/TimedOut.
No se afirma throughput lineal global ni aceptación física/RAM por estas cuotas.
Una optimización futura debe conservar la semántica y medir esta carga antes de
cambiar la estrategia. No se activó índice permanente ni servicio externo.

Referencias primarias consultadas: [DFA y límites de capacidad](https://docs.rs/regex-automata/0.4.18/regex_automata/dfa/regex/struct.Regex.html),
[configuración de construcción](https://docs.rs/regex-automata/0.4.18/regex_automata/dfa/dense/struct.Config.html).
Implementación nueva del adaptador; no port de ecode. Licencias MIT/Apache y
avisos de crates se incluyen en el paquete local. El lock añade también
aho-corasick/memchr como dependencias opcionales resueltas; el build observado
de esta configuración compiló regex-syntax y regex-automata.

## Evidencia de cierre

124 casos distintos observados aprobados: seis nuevos de regex (incluye oráculo
plano frente a fragmentos), nueve S0, cinco TUI multilínea (uno nuevo), ocho
proyecto, cinco paralelismo y91 libapp. Dos fixtures junction y seis fixtures
Git fallaron por restricciones locales de creación/acceso; no están aprobados.
Dos helpers subprocess permanecieron ignorados. No suite workspace completa
aprobada. [Metadatos, hashes y nombres](evidence/m2-regex-2026-11-03-metadata.json).

Fmt, Clippy workspace/all-targets y build release workspace bins/examples
aprobaron offline. Los logs conservan el primer fmt-check pendiente de aplicar
formato y su cierre correcto. Fallos iniciales de integración: parámetro app del
renderer, reinicio de selección con igual query y un import de fixture corregidos.
ConPTY corrigió oráculos de Ctrl+H, etiqueta/colisión del menú y mensaje transitorio;
no se cambió undo ni producto para satisfacerlos. [Corrida final](evidence/m2-regex-2026-11-03-conpty-5.json):
cuatro flujos reales, salida0,21301 bytes VT, fixture intacto: documento, reemplazo
literal, undo único y búsqueda de proyecto con filtros, sin otra tecla.

[Paquete local verificado](evidence/m2-regex-2026-11-03-package.json):
`work/packages/CIDE-windows-dev-regex-2026-11-03.zip`,5363246 bytes,
76 miembros; CRC/hashes verificados. SHA256
`b34cfee2d4135df4078c4da5b3937c093cabc68a7057ba7836453c56ff84231e`.
Continúan dos avisos GNU de targets no usados pendientes; no certificación de
distribución. No pantalla física, OS clipboard, AVM ni cierre global de TUI/M2A.

Siguiente bloque autorizado: completar comparación de los dos documentos abiertos
sin Git desde el checkpoint conservado, revisando su integración sobre el fuente
actual; construir ambas rutas de datos y la TUI antes de validar. Después siguen
el inventario de teclado/pendientes universales y proveedores según sus hitos.
