# Contrato de interacción de la TUI

Este documento es normativo para `jc-tui`. La TUI todavía no existe: primero se
formalizan comandos, foco, menús y atajos en `jc-app` para evitar que cada widget
invente su propio comportamiento.

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
| Delete line | Ctrl+Shift+K |
| Move line | Alt+Up / Alt+Down |
| Toggle line comment | Ctrl+/ |

Movimiento de cursor, selección con Shift, Home/End y edición de texto se definirán en
el input policy del editor; no se implementan en M0.5 porque dependen del modelo de
coordenadas/encoding de M1.

## Buscar

| Acción | Atajo |
|---|---|
| Find | Ctrl+F |
| Replace | Ctrl+H |
| Next / Previous | F3 / Shift+F3 |
| Find in Project | Ctrl+Shift+F |
| Replace in Project | Ctrl+Shift+H |

En la barra Find: `Enter` siguiente, `Shift+Enter` anterior, `Alt+C` case,
`Alt+W` whole word, `Alt+R` regex y `Esc` cierra.

## Sidebar y panel inferior

| Vista | Atajo |
|---|---|
| Explorer | Ctrl+Shift+E |
| Database / Schema | Ctrl+Shift+D |
| Git | Ctrl+Shift+G |
| Toggle bottom panel | Ctrl+J |
| Terminal | Ctrl+` |

Dentro del panel inferior exclusivamente:

`Alt+1 Problems`, `Alt+2 Output`, `Alt+3 Search Results`, `Alt+4 Terminal`.

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
- `Tab` plano no es un shortcut global ni del editor;
- los shortcuts de Progress se deshabilitan fuera de contexto;
- una pestaña sucia exige una decisión antes de cerrarse;
- los overrides contextuales son comprobables y explícitos.
