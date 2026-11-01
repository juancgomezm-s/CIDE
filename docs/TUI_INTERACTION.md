# Contrato de interacción de la TUI

Este documento es normativo para `jc-tui`. La TUI ya existe como frontend en desarrollo y debe obedecer los comandos, foco, menús y atajos definidos en `jc-app`; ningún widget puede inventar un comportamiento paralelo.

## Principios

1. **Los atajos de acciones son datos.** Un botón, opción de menú, palette y tecla
   invocan el mismo `CommandId`.
2. **El foco es explícito.** Las áreas primarias son `Editor`, `Sidebar` y
   `BottomPanel`. Menú, búsqueda, diálogos y palette son overlays transitorios.
3. **`Esc` siempre retrocede hacia el editor.** Primero cierra el overlay superior;
   después vuelve de sidebar/panel al editor; en editor sin overlay no hace nada.
4. **`Tab` no cambia de panel.** En el editor será indentación; en formularios,
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

## Menús

Barra superior prevista:

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

El movimiento, la selección y la edición pasan por Workbench con las coordenadas/encoding de M1. Tab/Shift+Tab son acciones de sangría solamente en Editor; F6 mantiene el control del foco. La TUI no mantiene otro buffer ni otro undo.

## Buscar

| Acción | Atajo |
|---|---|
| Find | Ctrl+F |
| Replace | Ctrl+H |
| Next / Previous | F3 / Shift+F3 |
| Find in Project | Ctrl+Shift+F |
| Replace in Project | Ctrl+Shift+H |
| Go to Line (pendiente) | Ctrl+G |
| Find selected next/previous (pendiente) | Ctrl+F3 / Ctrl+Shift+F3 |

En la barra Find: `Enter` siguiente, `Shift+Enter` anterior, `Alt+C` case,
`Alt+W` whole word, `Alt+R` regex y `Esc` cierra.

## Sidebar y panel inferior

| Vista | Atajo |
|---|---|
| Explorer | Ctrl+Shift+E |
| Database / Schema | Ctrl+Alt+D |
| Git | Ctrl+Shift+G |
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

La paleta PDSOE solo es referencia funcional; ver [alcance de comandos](ABL_EDITOR_COMMAND_SCOPE.md). El modo columna debe modelar selección rectangular real por columna visual y no una selección lineal decorativa. El plan de búsqueda prioritaria se describe en [Search Plus](M2_SEARCH_PLUS.md).

Compatibilidad terminal: Ctrl+I puede equivaler a Tab; Ctrl+Shift+/ y Alt+Shift+flechas pueden no llegar diferenciados. Cada acción debe seguir disponible mediante menú o Command Palette cuando el terminal no la reporte.

## Grupos de edición, navegación y Git — incremento M2 (propuesto)

Existe el modelo `PaneManager` en `jc-app`, pero los atajos de esta tabla **aún no están registrados** hasta que la TUI tenga handlers y renderizado real. No presentarlos como funciones operativas.

| Acción futura | Tecla propuesta |
|---|---|
| Split editor into two groups | Alt+Shift+2 |
| Return to one group | Alt+Shift+1 |
| Focus primary / secondary group | Ctrl+1 / Ctrl+2 |
| Pin/unpin active reference | Ctrl+Alt+P |
| Link/unlink scrolling | Ctrl+Alt+L |
| Navigate back / forward | Alt+Left / Alt+Right |
| Quick Open File | Ctrl+P |
| Go to Symbol | Ctrl+R |
| Go to Line | Ctrl+G (ya reservado) |
| Go to Definition / Peek | F12 / Alt+F12 |
| Find References | Shift+F12 |
| Manual completion | Ctrl+Space |
| Git sidebar | Ctrl+Shift+G (ya reservado) |

El anclaje impide que una navegación automática sustituya la referencia en el grupo anclado; scroll vinculado y navegación atrás/adelante son estados independientes. Abrir al lado usa otro grupo sin duplicar documento. Para terminales que no reportan Ctrl+1/2, Ctrl+Space u otras teclas, la Command Palette provee acceso. Git no ejecuta acciones remotas sin consentimiento y la resolución de conflictos se diseña como merge de tres vías.

Contrato completo: [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).
