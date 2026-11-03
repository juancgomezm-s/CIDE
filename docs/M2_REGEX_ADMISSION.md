# M2A — Admisión de regex compartido

Estado: auditoría y contrato previo, **sin motor regex integrado ni flag habilitado**.
Join/Split quedó cerrado en `7f42e85`; el rectángulo acotado en `3643902`. No se
reabren sus perfiles o suites por este diseño.

## Decisión que falta

La instrucción vigente de continuidad exige usar capacidades existentes «sin
instalar dependencias». El lock/cache local no contiene un motor regex. Por ello
este diseño no autoriza descargar crates ni sustituir el matcher por un regex
casero. Falta decidir si se permite incorporar una dependencia Rust mantenida,
fijada en Cargo, sin instalación del sistema. Sin esa autorización regex continúa
deshabilitado y se puede avanzar en otro pendiente M2A.

El candidato para evaluación es **regex-automata 0.4.18**, verificado en su
[documentación primaria](https://docs.rs/regex-automata/0.4.18/regex_automata/).
No está instalado, portado ni validado con este workspace. La versión final,
features, transitivas, licencia/MSRV, coste de compilación y compatibilidad offline
se comprobarán antes de aceptarlo. No se cambia Cargo.toml/lock en esta admisión.

## Por qué no basta añadir un toggle o llamar find_iter

`text_search::Pattern/Scanner` comparten KMP literal entre documento y proyecto.
S0 recibe rangos prestados y avanza por turnos; S1 usa el mismo scanner dentro del
executor por archivos. Los resultados y reemplazos dependen de rangos originales,
revisión/encoding y estado Complete, no de offsets de un String reconstruido.
Ese límite debe conservarse al añadir otra clase de patrón.

Upstream garantiza búsquedas con coste O(m·n), donde m depende del patrón y n del
texto; esto no proporciona por sí solo una interrupción dentro de una llamada
monolítica. [Contrato upstream](https://github.com/rust-lang/regex). Su módulo DFA
advierte que compilar puede costar tiempo/espacio exponencial y que no extrae
submatches de grupos. [Límites de DFA](https://docs.rs/regex-automata/0.4.18/regex_automata/dfa/).
No se elegirán DFA completo, backtracking o APIs sin puntos de comprobación solo
porque un ejemplo sea rápido. Timeout después de retornar no es cancelación real.

## Contrato de construcción propuesto

1. Un PatternKind explícito diferencia Literal y Regex en el engine compartido;
   literal permanece con su ruta optimizada y semántica actuales. El compilador
   regex produce programa inmutable con cuotas de patrón, anidación, repetición,
   estados y capturas. Probar primero compilación Thompson NFA y una ejecución
   cooperativa que contabilice transiciones, closures y slots, con presupuesto
   por turno y consulta. No se admite NFA sin límites por ser no-backtracking.
2. El runtime debe poder ceder a mitad de un escalar/closure sin duplicar matches
   ni perder prioridad de alternativas, greedy/lazy o capturas. La admisión se
   descarta si la API elegida no permite respetar esos contratos. Se puede usar
   un motor upstream como oráculo de tests; no otro motor editable en producción.
3. DecodedScalars continúa siendo la frontera de encoding. Byte original, byte
   UTF-8 de ejecución, ordinal escalar, LSP y celda visual son distintos. No crear
   un mapa denso ni String de todo el documento para buscar. Los matches/capturas
   deben volver a source ranges exactos, con origen de línea/columna escalar.
   BOM inicial fuera de texto, BOM intermedio literal, CRLF sin extremos internos.
4. Bytes inválidos no se convierten a U+FFFD buscable. Definir y probar barrera de
   matches con contexto real de anchors/word boundaries, o declarar expresamente
   no admitida esa fuente antes del scan; no segmentar texto y hacer que cada
   segmento parezca comienzo/fin de documento. Sin normalización NFC/NFD/EOL.
5. Full folding literal Unicode17 se conserva. No plegar la sintaxis regex como
   texto ni suponer que folding simple de un engine equivale al full fold actual.
   Deben figurar modo/semántica de case, versión Unicode y whole-word/anchors.
6. Before/after de coincidencias vacías debe garantizar progreso por frontera
   textual válida, incluido EOF, y un número acotado de resultados. No pasar por
   alto zero-width ni avanzar un byte dentro de UTF-8/CRLF para evitar loops.
7. Capturas y reemplazos tienen contrato previo: elegir sintaxis de referencias,
   escapes y grupos ausentes; compilar/validar el template una vez, acotar expansión
   y codificar estrictamente antes del batch. `$1` literal actual nunca cambia de
   significado sin modo Regex explícito. No habilitar reemplazo regex sin ese
   contrato o confundir el texto de un grupo con una instrucción ejecutable.
8. S0 mantiene entrega cooperativa y vigente; S1 mantiene Arc de patrón compartido,
   estado de scanner por archivo/helper, cuotas globales, backpressure y orden.
   Ningún worker escribe al documento o disco. Preview/reemplazo usan Complete y
   versiones/huellas vigentes con las transacciones existentes; parciales, Limited,
   Cancelled/TimedOut no autorizan replace-all.

La auditoría encontró además una ruta concreta que debe migrarse: el preview de
proyecto compone actualmente `-old` con la consulta literal. En regex debe mostrar
el contenido real encontrado, no el patrón: capturar un prefijo con cargo de
resultados o releer/revalidar al preparar el preview. No habilitar reemplazo regex
dejando esa composición vigente. Modo/flags deben entrar en historial, generación
y diagnósticos de compilación tipados, además de los parámetros del scanner.

Para un primer reemplazo regex puede conservarse reemplazo literalmente textual,
declarado como tal, antes de añadir templates de capturas. La decisión no permite
interpretar `$1` silenciosamente. Si se admiten coincidencias vacías, deben migrar
también navegación F3/Shift+F3 y reemplazo para no volver indefinidamente al mismo
offset; rechazar patrones anulables explícitamente es otra limitación posible,
no equivalente a soporte general. Ambos puntos deben resolverse antes de activar.

La API de bajo nivel permite explorar estados/transiciones de autómatas, pero
esto sigue siendo una hipótesis de integración, no una implementación cancelable
ya existente. [API primaria](https://docs.rs/regex-automata/0.4.18/regex_automata/dfa/trait.Automaton.html).

## Coste y salida del bloque amplio

Beneficio esperado: parser/compilación y semántica mantenidos por upstream,
evitando inventar un dialecto reducido para habilitar una casilla. Costes por medir:
código/tablas Unicode, NFA y caché por consulta, estados/capturas por helper,
compilación y adversarios sin coincidencias. Esos costes se suman a snapshots,
resultados, clipboard y editor; no hay promesa de RAM global inferior a50MiB.

Antes de activar UI: engine y reemplazos compartidos construidos, límites efectivos,
oráculo independiente, corpus Unicode/legacy/invalid/CRLF, queries adversarias y
vacías, cancelación a mitad de trabajo, stale/undo, cuotas y UI completa. Medir el
nuevo servicio al cierre; no repetir CPU/índice ya cerrados sin pregunta material.
Sin CI, push, AVM, GUI, indexador permanente, subprocesses ocultos o dependencia
automática del rg.exe del host de desarrollo. No hay nuevas pruebas/benchmarks
de regex por haber escrito esta admisión.
