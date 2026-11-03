# Contrato de interacción de la TUI

Este documento es normativo para `jc-tui`. La TUI ya existe como frontend en desarrollo y debe obedecer los comandos, foco, menús y atajos definidos en `jc-app`; ningún widget puede inventar un comportamiento paralelo.

El estado descrito corresponde a los flujos conectados actuales. Los checkpoints
históricos que aún llaman pendientes a S0, rectángulos acotados o Git read-only no
sustituyen este contrato. La evidencia y sus límites se consultan en los documentos
de cada bloque y en [VALIDACION.md](VALIDACION.md); registrar una tecla no acredita
por sí solo ejecución, latencia física ni paridad con una GUI.

## Principios

1. **Los atajos de acciones son datos.** Un botón, opción de menú, palette y tecla
   invocan el mismo `CommandId`.
2. **El foco es explícito.** Las áreas primarias son `Editor`, `Sidebar` y
   `BottomPanel`. Menú, búsqueda, diálogos y palette son overlays transitorios.
3. **`Esc` respeta el contexto.** Primero cierra/cancela el overlay superior;
   sin overlay puede cancelar la búsqueda contextual o el rectángulo. Desde otra
   área vuelve al editor cuando no consume la tecla una cancelación disponible.
4. **`Tab` no cambia de panel.** En el editor es indentación; en formularios,
   navegación de campos. `F6`/`Shift+F6` cambian áreas primarias.
5. **El contexto decide disponibilidad, no el widget.** Por ejemplo, F7 apunta a
   `ProgressCheck`, pero se deshabilita fuera de un documento ABL con perfil
   Progress configurado.
6. **Los shortcuts contextuales pueden sombrear un global conscientemente.**
   Ejemplo: `Alt+1` selecciona la primera pestaña desde el editor, pero en el panel
   inferior selecciona `Problems`. El keymap rechaza duplicados dentro del mismo scope.
7. **Toda función importante debe ser accesible sin ratón.** El ratón puede añadir
   conveniencia, nunca ser el único camino.

## Foco

Orden de `F6` con todas las áreas visibles:

`Editor → Sidebar → BottomPanel → Editor`

`Shift+F6` recorre el orden inverso. Áreas ocultas no participan. Mientras exista un
overlay, F6 no cambia el foco primario.

`Esc` sigue esta pila:

`Dialog/Palette/Search/Menu → área previa → Editor`

En Editor, el comando central `SearchCancel` cancela el scan documental y la
selección rectangular. En BottomPanel, `Esc` puede cancelar una búsqueda de
proyecto activa antes de volver al editor. Cancelar el rectángulo mediante
Edit/Palette no cancela también la búsqueda documental.

## Menús

Barra superior actual:

`File  Edit  Search  Project  Build  Tools  View  Help`

Aceleradores:

| Menú | Tecla |
|---|---|
| File | Alt+F |
| Edit | Alt+E |
| Search | Alt+S |
| Project | Alt+P |
| Build | Alt+B |
| Tools | Alt+T |
| View | Alt+V |
| Help | Alt+H |

Dentro del menú, flechas navegan, `Enter` activa y `Esc` cierra. Esas teclas son
semántica de navegación del control, no shortcuts de negocio duplicados.

El texto de shortcut mostrado junto a cada opción se obtiene de `KeyMap`; nunca se
escribe a mano en el renderer del menú.

## Archivo y pestañas

| Acción | Atajo |
|---|---|
| New | Ctrl+N |
| Open | Ctrl+O |
| Open Folder | Ctrl+Shift+O |
| Save | Ctrl+S |
| Save As | Ctrl+Shift+S |
| Save All | Ctrl+Alt+S |
| Close tab | Ctrl+W |
| Reopen closed tab | Ctrl+Shift+T |
| Exit | Alt+F4 |
| Next tab | Ctrl+Tab / Ctrl+PageDown |
| Previous tab | Ctrl+Shift+Tab / Ctrl+PageUp |
| Tabs 1–8 | Alt+1 … Alt+8 |
| Last tab | Alt+9 |

Una pestaña modificada se marca con `*`. `Close` sobre una pestaña sucia no la
elimina: abre `Unsaved changes` con `S = Save`, `D = Don't Save`, `Esc = Cancel`.

## Editor

| Acción | Atajo |
|---|---|
| Undo / Redo | Ctrl+Z / Ctrl+Y |
| Cut / Copy / Paste | Ctrl+X / Ctrl+C / Ctrl+V |
| Select all | Ctrl+A |
| Duplicate selection/line | Ctrl+D |
| Delete line | Ctrl+Shift+D |
| Move line | Alt+Up / Alt+Down |
| Toggle Comment (ABL usa /* ... */) | Ctrl+/ |
| Toggle Block Comment | Ctrl+Shift+/ |
| Indent / Outdent | Tab / Shift+Tab |
| Join Lines | Ctrl+Shift+J |
| Split Line | Enter |
| Iniciar/extender rectángulo por celda visual | Alt+Shift+flechas |

El movimiento, la selección y la edición pasan por Workbench con las coordenadas/encoding de M1. Tab/Shift+Tab son acciones de sangría solamente en Editor; F6 mantiene el control del foco. La TUI no mantiene otro buffer ni otro undo.

### Join Lines y Split Line

Ambas acciones están en Edit y Palette. `Enter` en Editor resuelve
`EditSplitLine`, el mismo comando que el menú; conserva la semántica de Enter en
diálogos, palette, sidebar y paneles. `Ctrl+Shift+J` une líneas en Editor;
`Ctrl+J` sigue siendo el atajo global del panel inferior.

Sin selección, Join une la línea actual con la siguiente línea real; con selección
lineal une las líneas tocadas y excluye la última si el extremo termina justo en
su comienzo. Una selección dentro de una sola línea es no-op. Retira EOL internos
y la indentación ASCII inicial de las líneas siguientes, insertando un espacio
cuando hace falta separar contenido. Conserva BOM, indentación de la primera
línea y EOL final, sin consumir el registro vacío sintético de EOF. Split usa el
EOL preferido en el caret o para reemplazar la selección lineal. Son operaciones
textuales universales, sin reglas sintácticas por lenguaje.

Una acción efectiva publica una sola edición/undo compartida por las vistas.
Join/Split están deshabilitados con rectángulo activo y el modelo también los
rechaza. Límites y costes pendientes: [M2_LINE_COMMANDS.md](M2_LINE_COMMANDS.md).

### Selección rectangular acotada

Existe selección rectangular real por columnas visuales, de una acción, con TAB
cada cuatro celdas y espacio virtual tras EOL. Edit/Palette ofrecen extensiones,
Cancel y `Rectangle: Select From Caret/Selection (single-cell text)` para convertir
extremos lineales existentes o iniciar sin ancho en el caret. Alt+Shift+flechas
inicia/extiende cuando el terminal entrega esas combinaciones. El modo requiere
foco Editor; ancho cero muestra cursor sin pintar selección.

El repertorio admitido es ASCII imprimible, U+00A0…U+00FF excepto soft hyphen y los
19 signos/símbolos CP1252 enumerados en el modelo. No admite Unicode general,
grafemas, combinantes, CJK, emoji o controles. Se valida la línea completa afectada
y se rechaza lo no admitido antes de activar/extender o editar, aunque quede fuera
de las columnas seleccionadas. Bytes, escalares y columnas visuales siguen siendo
coordenadas diferentes.

Copy/Cut usan clipboard **interno** tipado por filas; TAB completos/parciales se
copian como espacios y el espacio virtual se rellena. Texto, Tab y paste reemplazan
filas mediante una transacción; una fila se repite o varias deben corresponder a
la altura exacta. Paste rectangular en un caret usa líneas existentes, sin crear
líneas después de EOF. Se conservan encoding, BOM y separadores originales. Una
edición efectiva sale del rectángulo; no es multicaret persistente. Delete y
Backspace sin ancho borran una celda por fila, sin unir líneas. Borrar solo espacio
virtual es no-op. Enter, outdent y comandos de línea/comentario requieren cancelarlo.

Cada vista conserva su geometría independiente. Cambios de contenido o encoding
invalidan los rectángulos afectados en ambas vistas; el renderer solo consume
filas visibles preparadas, con TAB expandido y padding, y no muta el modelo.
Cuotas y evidencia: [M2_RECTANGULAR_SELECTION.md](M2_RECTANGULAR_SELECTION.md).

## Buscar

| Acción | Atajo |
|---|---|
| Find | Ctrl+F |
| Replace | Ctrl+H |
| Next / Previous | F3 / Shift+F3 |
| Find in Project | Ctrl+Shift+F |
| Replace in Project | Ctrl+Shift+H |
| Go to línea[:columna escalar] | Ctrl+G |
| Find selected next/previous | Ctrl+F3 / Ctrl+Shift+F3 |

En la barra Find: `Enter` siguiente, `Shift+Enter` anterior, `Alt+C` case,
`Alt+W` whole word y `Esc` cancela el scan pendiente y cierra.
Regex permanece deshabilitado hasta incorporar un motor con presupuesto real;
`Alt+R` está registrado en Find/Replace para un comando deshabilitado y no cambia
flags ni resultados. No representa un toggle funcional.

S0 documental literal es cooperativo por rangos, con default full case folding
Unicode 17 C+F cuando la búsqueda es insensible a mayúsculas; no aplica normalización
NFC/NFD ni elimina acentos. `Alt+C` activa comparación sensible y `Alt+W` palabra
completa. `Alt+Up/Down` recuperan el historial de consulta/flags. Ctrl+F3 y
Ctrl+Shift+F3 usan una selección lineal existente: no inventan una palabra sin
selección ni convierten un rectángulo en un rango lineal.

Ctrl+G ya está implementado: acepta línea y columna **escalares** de base uno,
rechaza cero/overflow/fuera de rango y no sitúa el caret dentro de CRLF. No son
columnas visuales de TAB/rectángulo ni posiciones LSP UTF-16. Los resultados llevan
revisión/encoding/vista; consultas parciales, limitadas o canceladas no se usan
como reemplazos completos. Ver [S0_DOCUMENT_SEARCH.md](S0_DOCUMENT_SEARCH.md) y
[decoder prestado](S0_DECODER_OPTIMIZATION.md).

Search Plus de proyecto conserva búsqueda literal **sensible a mayúsculas**;
no hereda automáticamente el default fold de S0. Ofrece lotes, cancelación,
preview de reemplazo y cuotas globales. Su paralelismo por archivos es configurable
con `--search-workers 1..4` (default 2, coordinador incluido), con el límite efectivo
de CPUs disponibles documentado en [CPU_RUNTIME_OPTIONS.md](CPU_RUNTIME_OPTIONS.md).
No implica un indexador residente. Ver [S1_PROJECT_SEARCH.md](S1_PROJECT_SEARCH.md).

## Sidebar y panel inferior

| Vista | Atajo |
|---|---|
| Explorer | Ctrl+Shift+E |
| Database / Schema | Ctrl+Alt+D |
| Git diff del archivo activo | Ctrl+Shift+G |
| Toggle bottom panel | Ctrl+J |
| Terminal | Ctrl+` |

Dentro del panel inferior exclusivamente:

`Alt+1 Problems`, `Alt+2 Output`, `Alt+3 Search Results`, `Alt+4 Terminal`.

### Explorer

Cuando la vista Explorer tiene un proyecto abierto:

- `Up/Down`: mover selección;
- `Home/End`: primero/último elemento;
- `Right`: entrar en el directorio seleccionado;
- `Left` o `Backspace`: subir un directorio sin poder abandonar la raíz del proyecto;
- `Enter`: abrir archivo o entrar en directorio;
- `R`: refrescar la carpeta actual.

Los directorios se ordenan antes que los archivos. La navegación resuelve rutas canónicas y rechaza salir de la raíz mediante enlaces o rutas indirectas.

### Git read-only

Ctrl+Shift+G abre el visor de diff del archivo activo. View/Palette permiten
configurar el ejecutable instalado y elegir HEAD/index, refresh o cancelación;
la entrega se valida por revisión/encoding/vista/ruta/generación. El snapshot Git
es inmutable y separado del buffer sucio. Ver [entrega diff](GIT_DIFF_DELIVERY.md)
y [presentación diff](GIT_DIFF_PRESENTATION.md).

El estado del repositorio también tiene una lista propia en View/Palette:
flags XY, rutas escapadas, Up/Down/Home/End, refresh y cancelación. Es un resultado
de Git sobre disco/index, no un espejo de cambios sin guardar ni una acción de
abrir rutas arbitrarias del listado. Ver [GIT_STATUS_PRESENTATION.md](GIT_STATUS_PRESENTATION.md).
Stage/commit/push/pull/merge y resolución de conflictos siguen en M5; no hay una
acción de escritura o remota implícita al abrir estas vistas.

### Bottom panel

`Up/Down` y `Home/End` mueven la selección del panel. En `Search Results`, `Enter` lleva al match seleccionado y devuelve el foco al editor. Problems/Output conservan selección visual, pero la navegación a diagnóstico se habilitará cuando M3/M4 aporten ubicaciones estructuradas.

## Progress ABL

| Acción | Atajo | Condición |
|---|---|---|
| Run | F5 | documento ABL + perfil Progress |
| Check Syntax | F7 | documento ABL + perfil Progress |
| Compile | Ctrl+F7 | documento ABL + perfil Progress |
| Refresh DB Schema | Shift+F7 | proyecto + perfil Progress |

Los comandos existen aunque estén deshabilitados; menú, palette y keymap consultan la
misma regla. El refresco de esquema no requiere que el editor tenga un documento ABL
activo, pero sí un proyecto con perfil Progress.

## Command Palette

`Ctrl+Shift+P` abre la palette. Debe listar todos los comandos registrados y mostrar
si están deshabilitados. Una función nueva puede existir inicialmente solo en palette;
no es obligatorio asignarle un shortcut si eso crea conflictos.

## Precedencia de keymaps

1. Scope del overlay o área enfocada.
2. Scope `Global`.
3. Sin coincidencia: la tecla vuelve al input policy del control.

Esto hace posible un override intencional como `Alt+1` en `BottomPanel` sin crear dos
implementaciones distintas de pestañas o paneles.

## Tests que deben permanecer verdes

- no existen dos comandos distintos con la misma tecla en el mismo scope;
- todos los bindings apuntan a comandos registrados;
- todos los items de menú apuntan a comandos registrados;
- el menú obtiene el shortcut del mismo keymap que usa el teclado;
- `Esc` cierra overlays antes de volver al editor;
- F6 solo visita áreas visibles;
- Tab y Shift+Tab no son atajos globales de foco: se reservan al scope Editor para sangría;
- los shortcuts de Progress se deshabilitan fuera de contexto;
- una pestaña sucia exige una decisión antes de cerrarse;
- los overrides contextuales son comprobables y explícitos.

## Productividad ABL, modo columna y búsqueda

La paleta PDSOE solo es referencia funcional; ver [alcance de comandos](ABL_EDITOR_COMMAND_SCOPE.md).
El rectángulo acotado descrito arriba ya es funcional; el modo columna Unicode
general y el cierre completo de Search Plus/M2A siguen pendientes. El plan de
búsqueda se conserva en [Search Plus](M2_SEARCH_PLUS.md), distinguiendo sus reservas
históricas de la implementación literal actual.

Compatibilidad terminal: Ctrl+I puede equivaler a Tab; Ctrl+Shift+/ y Alt+Shift+flechas pueden no llegar diferenciados. Cada acción debe seguir disponible mediante menú o Command Palette cuando el terminal no la reporte.

## Grupos de edición — incremento M2 implementado

Estos comandos ya usan `PaneManager`, handlers y renderizado TUI. Se muestran en View y Command Palette; al enfocar un grupo desde otra área, el foco primario vuelve al editor.

| Acción | Tecla registrada |
|---|---|
| Split editor into two groups | Alt+Shift+2 |
| Return to one group | Alt+Shift+1 |
| Focus primary / secondary group | Ctrl+1 / Ctrl+2 |
| Pin/unpin active reference | Ctrl+Alt+P |
| Link/unlink scrolling | Ctrl+Alt+L |
| Navigate back / forward | Alt+Left / Alt+Right |
| Open to Side (editor o archivo seleccionado en Explorer) | Ctrl+Enter |

Con menos de 65 celdas disponibles en el área editorial se conserva un solo grupo visible. Focus Group 1/2 cambia el grupo mostrado, preservando el estado del otro. Pin evita sustitución por Open y Open to Side; seleccionar explícitamente una pestaña puede cambiar el documento anclado. Linked Scroll solo vincula desplazamientos verticales, sin sincronizar selecciones/carets.

## Navegación de código y completado — reservas pendientes

Estas teclas siguen como reservas; no se registran handlers ficticios:

| Acción futura | Tecla propuesta |
|---|---|
| Quick Open File | Ctrl+P |
| Go to Symbol | Ctrl+R |
| Go to Definition / Peek | F12 / Alt+F12 |
| Find References | Shift+F12 |
| Manual completion | Ctrl+Space |

Ctrl+G y Git read-only ya están descritos como implementados, no como reservas.
El anclaje impide que una navegación automática sustituya la referencia en el
grupo anclado; scroll vinculado y navegación atrás/adelante son estados
independientes. Abrir al lado usa otro grupo sin duplicar documento. La Palette
ofrece los comandos registrados cuando el terminal no entrega sus atajos; no
convierte una reserva sin handler en una función real.

Contrato completo: [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).

## Evidencia y límites de este contrato

Registro y scopes: [command.rs](../crates/jc-app/src/command.rs),
[keymap.rs](../crates/jc-app/src/keymap.rs) y [menu.rs](../crates/jc-app/src/menu.rs).
Las regresiones de interacción están en
[keyboard_matrix_tests.rs](../crates/jc-tui/src/app/keyboard_matrix_tests.rs),
[rectangle_tests.rs](../crates/jc-tui/src/app/rectangle_tests.rs) y
[line_command_tests.rs](../crates/jc-tui/src/app/line_command_tests.rs). Los resultados
observados y el alcance de cada cierre pertenecen a [VALIDACION.md](VALIDACION.md)
y a los documentos de los bloques; esta reconciliación documental no añade una
ejecución ConPTY ni repite pruebas.

Screen usa celdas escalares, no un motor general de grafemas/anchos. Las pruebas
45/140 y resize acreditan composición del modelo, sin certificar glyph widths,
latencia tecla-pantalla, DPI, IME o accesibilidad física. No hay GUI aceptada ni
integración del clipboard del sistema operativo por describir Cut/Copy/Paste
internos. Sublime Text conserva su papel de referencia visual, sin promesa de
paridad gráfica o funcional derivada de este contrato.
