# CIDE — Search Plus de proyecto nativo

Seguimiento: el [bloque GUI de reemplazo y Git](M2B_PROJECT_REPLACE_GIT.md) habilita
ahora reemplazo de proyecto explícito. Las exclusiones de abajo describen el cierre
histórico de búsqueda, no la disponibilidad del ejecutable actual.

Este bloque conecta el S1 existente a la GUI Win32/GDI. No añade motor, índice,
regex ni ámbito de reemplazo. El reemplazo documental multilínea ya implementado
se conserva; reemplazo de proyecto GUI no se habilita por mostrar resultados.

Search / Search Plus: Find in Project, Palette y Ctrl+Shift+F abren Query/Include/
Extra Excludes. Tab cambia campo; flechas/Home/End/Backspace/Delete editan; Ctrl+U
vacía; Ctrl+V pega. F2 LF, F4 CRLF, F7 TAB y F8 CR permiten bloques exactos sin
ejecutar. Enter inicia un scan literal **sensible a mayúsculas**, con EOL exactos.
Los campos admiten4096 bytes UTF8 cada uno; paste excesivo se rechaza completo.
Globs se separan por coma, usan slash y conservan los límites S1: hasta16 patrones
de128 bytes, incluyendo exclusiones por defecto. No se deduce encoding legacy.

Resultados: Up/Down/Page/Home/End seleccionan, Enter abre el destino validado,
F5 refresca con la configuración admitida, F2 vuelve al borrador, Escape cancela
y oculta. Search / Project Search Results o Palette muestra el último resultado
sin iniciar otro scan. Los notices no son destinos; los resultados stale exigen
refresh. Guardar, editar o navegar sigue usando el Workbench único.

El controller S1 prefiere snapshots abiertos, incluidos cambios sin guardar;
admite2MiB por archivo/8MiB totales de abiertos y hasta128 snapshots con revisión/
encoding. El resto de cuotas, cancelación cooperativa y entrega por generaciones
son los de [S1](S1_PROJECT_SEARCH.md). Workers default2, configurables al arrancar
`cide-gui --search-workers 1..4`; el ejecutor mantiene su cap/headroom existente.
No pools anidados nuevos, watcher ni indexador permanente al abrir una carpeta.

La ventana acepta hasta2 lotes por TASKS_MESSAGE. Antes de preparar una escena,
validate_context retira filas abiertas obsoletas sin consumir completions; no
espera otra tecla ni acepta respuestas desde la pintura. Cambiar raíz limpia
controller, opciones y borrador; una respuesta anterior no repuebla la escena.
Recuperación de panic se conserva aun si cambió contexto. El callback Win32 solo
publica el aviso coalescido del HWND propio; no posee el modelo.

La presentación prepara como máximo32 filas y256 columnas ASCII por etiqueta.
Rutas permanecen PathBuf opacos en S1; la etiqueta relativa/escapada no se usa para
navegar. Unicode/control/bidi/bytes no UTF8 son escapes completos, no conversiones
con pérdida. Caret y campos siguen el editor UTF8 compartido; Ln/Col de resultados
son escalares, no columnas de display. GDI consume una escena propia de solo lectura.

Enter revalida sello de documento abierto o fingerprint/lectura confinada de
disco. open_project_file_at_version comprueba contenido/encoding antes de cambiar
catálogo, foco o historial. Un cerrado se lee una sola vez por handle confinado
con2MiB y reutiliza su fingerprint inicial. Si ya está abierto, recorre sus piezas
prestadas sin aplanar y verifica también el disco: seleccionar una primera vez no
convierte un resultado de disco en uno de snapshot. No recarga un buffer sucio ni
abre un path derivado de una etiqueta. Un conflicto permanece explícito.

## Evidencia del bloque

Se construyen campos, menús, contextos, resultados, transporte y casos E2E antes
de la ventana serial de checks. El smoke usa HWND/fixture propios y un archivo
auxiliar create_new para comprobar dirty/multilínea/notificación, navegación sin
reload, stale/refresh, cancelación sin resurrección, conflicto de disco y root
invalidation. El bitmap opcional es raster GDI, no pantalla física. Los resultados
observados se retienen junto al rebrand CIDE y sus iconos.

Cierre local:391 casos distintos aprobados (260 suites completas de la primera
corrida y131 de paquetes restantes; no una corrida391 sin interrupciones),2
benchmarks ignorados. Nueve nuevos: tres aperturas versionadas, tres resultados,
dos geometrías y un parser de workers. fmt/Clippy/release aprobados.41 controles
nativos aprobaron con0 errores de pintura y fixture13B intacto. Sin CI/push ni
dependencias nuevas. La ventana oculta y raster GDI no certifican pantalla física.

La validación detectó/corrigió colisión de entrypoint CLI, omisión de comprobar
disco al reutilizar un abierto, underflow de layout en cliente diminuto y título
nativo no establecido por WM_NCCREATE. El primer recorrido Save All dentro del
sandbox falló por filesystem; el mismo release pasó fuera del sandbox con fixtures
propios. Se conservan los fallos, sin describirlos como evidencia aprobada.

- [Reporte nativo41](evidence/cide-project-native-2026-11-02-native-report.json).
- [Hashes de fuentes/binarios y método](evidence/cide-project-native-2026-11-02-metadata.json).
- [Raster GDI de resultados](evidence/cide-project-native-2026-11-02-native-project-results.png).
- Logs crudos `evidence/cide-project-native-2026-11-02-*.log`.

Permanecen abiertos proyecto replace en GUI, regex, integración de proveedores,
clipboard físico, IME/DPI/accesibilidad y aceptación global RAM/pantalla. No se pide
validación manual intermedia al propietario ni se repiten perfiles ya cerrados.
