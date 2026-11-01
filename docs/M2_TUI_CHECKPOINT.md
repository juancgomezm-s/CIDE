# M2A — Checkpoint TUI

**Estado:** en desarrollo. Este documento registra implementación, no cierre de épica.

## Implementado

- `jc-tui` como frontend terminal basado en crossterm.
- Un único `Workbench` de `jc-app` posee documentos, cursor, selección, búsqueda, clipboard y undo/redo.
- Abrir, crear, Save, Save As, Save All, cerrar con decisión por cambios y reabrir pestaña.
- Edición byte-safe a través de M1: texto, newline, tab, backspace/delete, cursor y selección.
- Undo/redo, cut/copy/paste/select-all.
- Duplicate selection/line, Delete Line, Move Line Up/Down y Toggle Comment. Para ABL, Ctrl+/ inserta/retira /* ... */ en línea o selección; Ctrl+Shift+/ fuerza bloque. Tab/Shift+Tab gestionan sangría.
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

## Checkpoint: comandos de productividad ABL

- Ctrl+D = Duplicar y Ctrl+Shift+D = Eliminar línea; Database/Schema se reasignó a Ctrl+Alt+D.
- Tab = Indentar y Shift+Tab = Desindentar, solo con foco de editor.
- Ctrl+/ = Comentar/descomentar código; Ctrl+Shift+/ = Alternar bloque.
- ABL usa /* ... */ y el cambio se realiza sobre el Document, sin invocar Progress.
- Se añadieron tests de regresión para encoding legacy, BOM, selección multilineal y atajos.
- **La implementación aún no ha sido compilada en el entorno actual**; conforme D13, CI remoto se difiere a M5.
- Alcance detallado: [comandos ABL](ABL_EDITOR_COMMAND_SCOPE.md) y [Search Plus](M2_SEARCH_PLUS.md).

## Checkpoint de dos grupos — 2026-11-01

- Renderer TUI de Primary/Secondary, títulos con grupo activo, pin y scroll link. Dos columnas con al menos 65 celdas disponibles en el editor; por debajo se muestra solo el grupo activo sin destruir el otro.
- Cursor, anchor/selección y viewport separados por grupo. `EditorDocument` conserva el estado de trabajo de la vista activa por compatibilidad con los comandos existentes; al cambiar foco se guarda/restaura desde `PaneManager`. El documento y undo permanecen únicos.
- `ByteEdit` conserva únicamente rango reemplazado y longitud insertada; undo informa su geometría inversa. Workbench ajusta caret/selección inactivos e historiales por bytes. Los carets dentro de texto reemplazado colapsan al final del reemplazo; una inserción en su posición los desplaza a la derecha.
- Split/One Group, Focus Group 1/2, Pin/Unpin, Linked Scroll, Open to Side y Back/Forward registrados centralmente y accesibles por menú/palette/teclado.
- Open reutiliza un archivo ya abierto (ruta canónica), preservando bytes sucios e historial. Explorer admite Ctrl+Enter para abrir al lado; una referencia anclada no se sustituye por apertura automática/lateral.
- Scroll vinculado aplica el mismo delta vertical a la otra vista, limitado a sus líneas, sin mover su cursor. No es alineación por hunks de un diff.
- Cerrar una pestaña compartida limpia ambos grupos/historiales; cerrar el segundo grupo conserva sus documentos abiertos.
- Pruebas de core, Workbench y eventos/render TUI ejecutadas localmente. Smoke manual en terminal Windows de menús, split, foco, pin, apertura automática, back y salida; aún falta revisión completa de terminales/atajos. Evidencia en [VALIDACION.md](VALIDACION.md).
- Se corrigió el uso de `?` en un handler de Open Folder que retornaba `()`, el fondo transparente de overlays y el scroll de menús altos. Se aplicó rustfmt también al formato pendiente del checkpoint anterior.
- Navegación de includes/RUN, Outline, DiffModel, Git y autocomplete siguen pendientes; M2A no está cerrado.
- Documento de arquitectura detallado: [M2_NAVIGATION_PANES_GIT.md](M2_NAVIGATION_PANES_GIT.md).
- [Prompt de continuación desde el equipo](CONTINUAR_EN_EQUIPO.md) para completar la implementación y validarla localmente sin Actions.

## Pendiente para cerrar M2A

1. Find/Replace in Project real sobre la raíz abierta, con límites y exclusiones claras.
2. Modelo de tareas no bloqueante para Build/Progress que pueda alimentar Output/Problems sin ejecutar la integración real antes de M3.
3. Navegación estructurada desde Problems cuando exista `file + line + column`.
4. Modo columna rectangular auténtico con posiciones visuales, clipboard y undo transaccional.
5. Revisión manual en terminal real de todas las combinaciones de foco/atajos.
6. Ajustes acumulados de compilación/formato se endurecen en M5, conforme D13; no se consume GitHub Actions durante M2–M4.

## Restricciones deliberadas

- Database/Schema corresponde a M3/M4.
- Git read-only/diff es un incremento pendiente de M2; escritura Git y terminal embebida siguen diferidas.
- Regex sigue deshabilitado hasta tener un motor/contrato explícito.
- La TUI no tiene lógica de documentos independiente de `jc-app`.
- Este checkpoint no afirma CI verde; CI remoto está diferido hasta M5.
