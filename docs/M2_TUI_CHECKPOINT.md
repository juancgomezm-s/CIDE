# M2A — Checkpoint TUI

**Estado:** en desarrollo. Este documento registra implementación, no cierre de épica.

## Implementado

- `jc-tui` como frontend terminal basado en crossterm.
- Un único `Workbench` de `jc-app` posee documentos, cursor, selección, búsqueda, clipboard y undo/redo.
- Abrir, crear, Save, Save As, Save All, cerrar con decisión por cambios y reabrir pestaña.
- Edición byte-safe a través de M1: texto, newline, tab, backspace/delete, cursor y selección.
- Undo/redo, cut/copy/paste/select-all.
- Duplicate selection/line, Delete Line, Move Line Up/Down y Toggle Line Comment.
- Find/Replace de documento, flags case/whole-word, resultados y replace current/all.
- Tabs y shortcuts centralizados.
- Menús que muestran el shortcut derivado del `KeyMap`.
- Command Palette.
- Foco Editor/Sidebar/BottomPanel y overlays.
- Explorer de proyecto con navegación por teclado, refresh y restricción a la raíz.
- Bottom panel con selección; Search Results navega al match y devuelve foco al editor.
- Selección explícita de encoding legacy cuando M1 no puede determinarlo.

## Teclas del Explorer

| Tecla | Acción |
|---|---|
| Up / Down | seleccionar entrada |
| Home / End | primera / última |
| Right | entrar al directorio |
| Left / Backspace | subir directorio |
| Enter | abrir archivo / entrar directorio |
| R | refrescar |
| Esc | volver al editor según contrato de foco |

## Bottom panel

| Tecla | Acción |
|---|---|
| Up / Down | mover selección |
| Home / End | primera / última |
| Enter en Search Results | seleccionar match y volver al editor |
| Esc | volver al editor |

Problems y Output aún son texto sin ubicación estructurada. No fingimos navegación a línea/archivo hasta que el proveedor de tareas/Progress entregue diagnósticos tipados.

## Pendiente para cerrar M2A

1. Find/Replace in Project real sobre la raíz abierta, con límites y exclusiones claras.
2. Modelo de tareas no bloqueante para Build/Progress que pueda alimentar Output/Problems sin ejecutar la integración real antes de M3.
3. Navegación estructurada desde Problems cuando exista `file + line + column`.
4. Revisión manual en terminal real de todas las combinaciones de foco/atajos.
5. Ajustes acumulados de compilación/formato se endurecen en M5, conforme D13; no se consume GitHub Actions durante M2–M4.

## Restricciones deliberadas

- Database/Schema corresponde a M3/M4.
- Git y terminal embebida son servicios opcionales posteriores.
- Regex sigue deshabilitado hasta tener un motor/contrato explícito.
- La TUI no tiene lógica de documentos independiente de `jc-app`.
- Este checkpoint no afirma CI verde; CI remoto está diferido hasta M5.
