# Resaltado léxico compartido TUI/GUI — 2026-11-03

Prioridad explícita de Juan: integrar resaltado sintáctico en ambas interfaces.
Se inspeccionaron perfiles, tokenizadores, cachés, revisiones y renderers antes
del diseño: había18 perfiles y selección/búsqueda/diagnósticos, sin tokenizador
de fuente. Esta entrega es una capacidad léxica local, no semántica, parser,
compilación AVM ni cargador/proveedor externo. No añade dependencias.

## Estado del trabajo de comparación

El diff de documentos abiertos comenzó sobre66aa783 y se suspendió al recibir
esta prioridad. Se conservaron11 archivos, patch y manifiesto con hashes en
[checkpoint fuente](checkpoints/open-diff-before-syntax-66aa783.zip), con
[estado preciso](checkpoints/open-diff-before-syntax-66aa783.json). Controller y
admisión dual F3 estaban construidos; faltaban vista compartida, cableado final,
arneses y toda validación. No se afirma que ese diff funcione. El árbol fuente
se restituyó a66aa783 antes del resaltado; el diff Git existente sigue operativo.
El archivo es trabajo recuperable, no paquete de distribución ni componente activo.

## Capacidad y coordenadas

`jc-languages` declara `SyntaxProfile` en el perfil: ABL, Rust, JSON, JSONC o Plain.
Los otros perfiles conservan identificación/comentarios y usan texto plano para
esta capacidad. `jc-app` no consulta `if language == "abl"` ni inicia servicios.
El empaquetado futuro de capacidades especializadas de Progress conserva la
frontera de extensiones; este módulo no afirma que `progress.abl` esté operativo.

Un lexer streaming reconoce delimitadores ASCII sobre los bytes canónicos.
Emite `TokenSpan { source: Range<usize>, kind }`, ordenados y en coordenadas byte
de la revisión indicada, nunca columnas escalares/grafemas/display. El contenido
de strings/comments puede conservar bytes UTF8/legacy/invalid sin convertirlos
en otro documento. `SyntaxKind` distingue Plain, Keyword, String, Comment, Number,
Preprocessor y Operator; no se infieren tipos, símbolos, keywords abreviadas o BD.

ABL usa un catálogo explícito de keywords case-insensitive e incluye nombres
con guion; strings simples/dobles, comillas duplicadas, escape tilde y continuación
~LF/~CRLF; comentarios /* */ anidados, números, directivas y referencias entre
llaves marcadas léxicamente. Una comilla sin continuación se recupera en EOL:
no se pinta el resto del archivo como string por un error aislado. `//` no se
habilita como comentario ABL de perfil sin confirmar la versión legacy real.
Las directivas/includes se identifican, no se expanden ni resuelven. No es una
gramática exhaustiva ni validación de OpenEdge10.0B/11.7.

JSON reconoce strings/escapes, números y true/false/null; JSONC añade comentarios.
Estos subconjuntos no validan documentos JSON ni reemplazan parsers. C#/JS/
Python/otros perfiles continúan Plain explícito, sin prometer soporte por extensión.

La ampliación posterior de Juan añade Rust al mismo núcleo y ambas interfaces:
keywords/literales, candidatos numéricos con bases/sufijos/exponente/underscores,
comentarios // y /* */ anidados, strings normales multilínea y raw r/br con hasta32
hashes. Superar ese límite declara Plain+limited, finito. Caracteres y escapes se
distinguen de lifetimes: char es String, lifetime es Preprocessor; las sondas tienen
longitud acotada a16 bytes. No se resuelven nombres/tipos, macros, atributos o semántica de
ediciones Rust, ni se promete gramática exhaustiva. ABL conserva sus reglas de
continuación propias. Referencias primarias: [tokens Rust](https://doc.rust-lang.org/reference/tokens.html),
[comentarios Rust](https://doc.rust-lang.org/stable/reference/comments.html).

Referencias léxicas primarias: [comentarios anidados](https://docs.progress.com/bundle/abl-reference/page/Multi-line-comments.html),
[continuación de strings](https://docs.progress.com/bundle/openedge-develop-abl-applications-128/page/An-ABL-procedure-consists-of-statements.html),
[tilde](https://documentation.progress.com/output/ua/OpenEdge_latest/dvref/~-special-character.html),
[comillas duplicadas](https://documentation.progress.com/output/ua/OpenEdge_latest/dvref/-22-special-character.html).
Los manuales no certifican ejecución en la instalación del usuario.

## Caché incremental y presupuesto

Cada EditorDocument posee una SyntaxCache única, compartida entre vistas. Su
identidad comprende revisión, encoding, perfil y ruta. Los cambios directos,
transacciones externas y undo/redo invalidan desde el primer byte cambiado,
conservando solo checkpoints de prefijo válidos. Links de revisión ausentes,
cambio de encoding/ruta/perfil o ABA no reutilizan un sufijo antiguo.

Se leen chunks/rangos del Document, sin decode/copia global por tecla ni texto
fuente retenido en otra caché editable. Un estado streaming acotado atraviesa
fronteras de piezas/chunks y conserva comentario/string/escape/palabra parcial.
Hasta64KiB de trabajo nuevo por llamada; dos grupos pueden consumir128KiB por
preparación. Saltar a contexto profundo retorna Plain+pending y continúa mediante
Wakeup/TASKS_MESSAGE o el loop TUI, sin otra tecla. `Workbench::continue_syntax`
avanza en el evento del propietario sobre destinos visibles aún vigentes, sin
depender de WM_PAINT. Cada evento puede avanzar dos documentos (128KiB), además
de hasta128KiB durante la preparación de dos vistas;64KiB es el límite por llamada.
Cambiar de destino deja de
programar ese trabajo visible; una edición invalida su cursor/contexto anterior.

La caché tiene128KiB lógicos incluyendo capacidades de tokens/checkpoints/ruta.
Máximos:256 checkpoints dispersos,1536 spans retenidos,128 filas por solicitud y
256KiB de ventana visible. Saturar ventana/tokens/contexto devuelve Plain+limited
finito, sin requeue infinito ni colores presentados como precisos. Al terminar,
repetir la misma ventana caliente no vuelve a escanear fuente; alternar ventanas
puede volver a recorrer una ventana acotada usando checkpoints. Las filas devueltas
poseen solo rangos/tipos acotados, no otra copia de los bytes fuente.

Se redistribuye la cuota de1MiB/documento: layout textual768KiB, invisibles128KiB
y sintaxis128KiB. Metadatos de líneas se contabilizan aparte como antes. Esto
no certifica RAM privada global, coste del layout frío o latencia física. Las
estadísticas léxicas separan bytes procesados por llamada, cursor y carga retenida.

## Presentación común y prioridades

TUI y GUI solicitan SyntaxRows durante preparación visible, luego componen sin
leer/ejecutar el lexer desde draw. La paleta RGB compartida es dato de jc-app;
TUI la traduce a VT truecolor y GUI a GDI. TUI respeta NO_COLOR; el arnés
ConPTY habilita colores solo en el entorno explícito de su propio hijo. Las
celdas GUI no añaden un offset fuente por celda ni otro buffer editable. Selección/búsqueda prevalecen sobre
sintaxis; marcadores de invisibles/invalid tienen estilo de diagnóstico; sintaxis
colorea el resto. Espacios virtuales de rectángulo no adquieren bytes ni tokens.
No cambian tabulaciones, caret, rangos copiados, hit-testing ni transacciones.

Pending muestra «Syntax preparing» y fallback «Syntax: plain»/presupuesto, sin
reescribir el estado de negocio. La salida terminal sigue incremental: cambios
de estilo forman parte del diff de celdas y se restauran colores/atributos al
volver a Normal y salir. GUI incorpora búsqueda S0 a la misma prioridad de fondo
de selección existente, por intersección byte, sin reinterpretar display columns.

## Validación

Construcción y cableado completos antes del cierre de pruebas. Pasaron35 casos
Rust distintos (36 líneas de aprobación: un caso profundo repetido tras corregir
la continuación),18 nuevos;11 checks HWND (4 nuevos),5 flujos ConPTY y3 rechazos
CLI. También fmt, Clippy workspace/all-targets y build release workspace con
binarios/examples, locales offline. No se ejecutó toda la suite runtime del workspace.

Los fallos iniciales están retenidos: dos errores de compilación de fixtures,
un oráculo VT que ignoraba NO_COLOR, dos lints de estilo, dos ejecuciones HWND
que revelaron dependencia indebida de pinturas para continuar contexto profundo.
Esta última corrigió producción mediante continuación en eventos del propietario.
En ConPTY, dos caracteres llegaron como dos eventos: se corrigió el arnés para
dos undos/redos y salida limpia, conservando la semántica de historial.

[Metadatos y hashes](evidence/m2-syntax-2026-11-03-metadata.json),
[HWND](evidence/m2-syntax-2026-11-03-native-report.json),
[ConPTY](evidence/m2-syntax-2026-11-03-conpty-report.json),
[paquete verificado](evidence/m2-syntax-2026-11-03-package.json).
La [captura GDI](evidence/m2-syntax-2026-11-03-native-syntax.png) muestra ABL en
dos vistas; Rust se comprobó con aserciones de celdas GUI y salida ConPTY, sin
atribuirle esa captura ABL. Fixtures y metadatos Git quedaron intactos.

Permanecen pendientes aceptación física/global de latencia y RAM, Unicode
general, IME/DPI/accesibilidad, clipboard externo, symlink y certificación de
distribución; dos avisos GNU pendientes en el paquete de desarrollo. No implica
aceptación completa de M2/M3, gramática o proveedor. CI dormido hastaM5.

Autorización posterior de Juan: incorporar/fijar una dependencia Rust mantenida
para regex real está autorizado para el siguiente bloque Search Plus. No se
descarga ni usa esa dependencia en este resaltado; regex sigue deshabilitado
hasta su implementación y validación con presupuestos. No pedir de nuevo esa
autorización ni interpretarla como instalación de programas del sistema.

Paquete local: `work/packages/CIDE-windows-dev-syntax-2026-11-03.zip`, 4284421 bytes; SHA256 `5ae95ac62f722bf26fc694de841a6e312f3d42a1391dae005c6d49ca1be84234`. CRC y hashes de los69 miembros verificados; canal de desarrollo, sin instalador ni publicación.

Actualización posterior: el checkpoint de diff se integró y validó en TUI sobre
regex/resaltado actuales; [estado vigente](M2_OPEN_DOCUMENT_DIFF.md). La descripción
de suspensión anterior registra el estado al entregar resaltado, no el actual.
