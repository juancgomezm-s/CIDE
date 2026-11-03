# M2B — Save All explícito

File / Save All, Palette y Ctrl+Alt+S usan un plan temporal de documentos
modificados, no de vistas. Cada DocumentId aparece una vez, con revisión y estado
new capturados. Se admiten1024 documentos como máximo antes de escribir; el plan
retiene un boxed slice de metadatos, sin paths, fuentes, sesiones ni undo clonados.

Para cada candidato, Workbench captura un SaveTarget con id/revisión/path/is_new,
con path hasta4096 unidades nativas. Un nuevo documento necesita un destino
explícito; no se escribe el nombre untitled generado al pulsar Save All. Su diálogo
identifica el documento y se ejecuta sin préstamo del modelo. Al volver se
revalidan revisión, identidad, path e is_new antes de FileSession.save_as.
Los existentes usan FileSession.save, con su conflicto externo/atomicidad/retries
vigentes. Save As normal ahora también captura su target antes del diálogo.

Un éxito actualiza título/dirty del tab del documento guardado sin activar otra
vista ni mover caret/selección/historial. Undo/redo del contenido permanece. El
plan avanza solo al acusar éxito exacto del candidato pendiente. No es una nueva
ruta de edición ni una transacción de guardado global del proyecto.

Cancelar un Save As detiene los restantes y deja los documentos no guardados
intactos. Un error/conflicto también detiene el plan. La barra de estado informa
cuántos se guardaron y cuántos quedan; los ya guardados no se revierten. No hay
reentrada de otro Save All mientras el actual esté abierto. Un destino existente
conserva el rechazo/no-clobber de Save As: no se introduce force overwrite.

La TUI conserva su flujo de nombres explícitos y usa el mismo save_active_as
compartido. Este incremento implementa la orquestación gráfica; no añade autosave,
watcher ni operaciones Git. Los nuevos vacíos sin modificaciones no entran en
Save All; File / Save/Save As permite guardarlos explícitamente.

## Validación del bloque

Se construyeron plan puro, target core, flujo nativo, menú/teclado/Palette y
regresiones antes de iniciar su ventana serial de validación. El smoke usa
HWND/fixture propios y decisiones Save As inyectadas únicamente para un destino
nuevo junto al fixture, o cancelación. No acepta un diálogo del usuario ni abre
otra aplicación. Comprueba guardado parcial/cancelación, destino explícito con
UTF8/EOL/undo/vistas intactos y rechazo a un cambio externo sobre ese archivo
exclusivo. Las decisiones físicas del usuario siguen pendientes para el final.

La evidencia observada se registra al cerrar; no inferir resultados por este plan.


## Cierre local — 2026-11-02

29 pruebas dirigidas jc-app (3 nuevas de SaveTarget +26 F1/Workbench) y16 de
jc-gui (3 nuevas del plan +13 previas) aprobaron.6 casos nuevos elevan el conjunto
histórico de376 a **382 distintos**; no se declara una corrida completa382.
Formato, Clippy workspace all-targets -D warnings y release workspace bins/examples
aprobaron. Se sustituyó el ejemplo de comando no conectado del smoke por Go to
Line/Column, porque Save All ya está implementado; solo rebuildGUI, sin repetir
suite global ni perfiles.

**32 checks nativos aprobaron**: los29 de los flujos anteriores más cancelación
después de guardar el existente, Save As de nuevo con byte/EOL/view/undo intactos
y conflicto externo que conserva el contenido del disco.0 errores de pintura,
fixture final13 bytes original. El segundo archivo propio queda con el sentinel
externo para acreditar que no fue sobrescrito. wall_ms=44 es duración funcional,
no latencia de pantalla. Decisiones de diálogo inyectadas bajo smoke, no se abrió
un diálogo de usuario ni clipboard real. Manual/global siguen para el final.

- [Reporte nativo](evidence/m2b-save-all-native-report-2026-11-02.json).
- [Logs de validación](evidence/m2b-save-all-validation-2026-11-02.log).
- [Hashes, fixtures y conteos](evidence/m2b-save-all-metadata-2026-11-02.json).

La cuota de path usa OsStr::len (representación nativa codificada), no columnas
visuales ni un conteo de caracteres Unicode. No se introdujeron dependencias,
CI/push ni mediciones globales. El siguiente bloque funcional del roadmap es
llevar Search Plus de proyecto al GUI, reutilizando el controller S1 ya validado.
