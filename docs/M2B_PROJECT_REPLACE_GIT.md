# GUI: reemplazo de proyecto y Git de solo lectura

Bloque M2B del plan original, posterior a la búsqueda gráfica S1. Se construyen
entrada, controladores, transporte y presentación juntos; la validación integral
se realiza al cerrar el bloque. No añade motor de búsqueda/diff, índice permanente,
dependencias, .NET, acciones Git con escritura ni servicios de lenguaje.

## Reemplazo explícito

Tras una búsqueda de proyecto completa, sin avisos ni resultados obsoletos,
Search / Replace in Project, Palette o Ctrl+Shift+H abren el reemplazo. El campo
admite4096 bytes UTF8; vacío elimina las coincidencias. Ctrl+V usa un handoff
explícito de clipboard, revalidado por raíz y generación del draft. Ctrl+U vacía;
F2 LF, F4 CRLF, F7 TAB, F8 CR y Shift+Enter LF conservan EOL exactos.

Enter **solo crea la vista previa** agrupada por archivo, con rangos y pares -/+.
Las flechas/Page/Home/End permiten revisarla; F2 vuelve al texto. Únicamente F9
o Ctrl+Enter confirman desde esa vista previa. Enter simple y confirmaciones
repetidas durante/después de aplicar no inician otra operación. Escape descarta
el preview sin escribir. Un cambio de revisión/encoding invalida el preview y
conserva el texto; cambiar raíz elimina todo plan pendiente.

La GUI mueve el mismo ProjectReplacePreview de S1, sin clonar documentos,
sesiones, historial ni planes. Se aplica como máximo **un archivo por TASKS_MESSAGE**,
se atienden input/pintura antes del siguiente y se informa por archivo en Output
y en el resumen visible. Escape cancela los restantes, conservando lo terminado.
Abiertos: revisión esperada, una transacción/undo, sin guardar. Cerrados: apertura
por handle confinado con fingerprint y cuota2MiB, seguida del guardado FileSession
con sus conflictos/reintentos/recuperación existentes. Se retiraron la lectura y
apertura genérica duplicadas de esa ruta. No-op no crea undo ni reescribe disco.

Se conservan las cuotas S1: proposals2MiB/archivo, entrada/salida8MiB cada una y
preview2MiB. Encoding estricto; un carácter no representable rechaza el preview.
No hay transacción ni undo global del filesystem. I/O/guardado del archivo actual
termina antes de atender Escape; el commit no es un CAS contra actores hostiles.

La presentación solo toma32 filas/256 columnas ASCII; las etiquetas nunca se
reinterpretan como rutas. Los paths del preview core se escapan desde sus bytes
nativos para no introducir conversiones lossy al extender su consumo gráfico.

## Estado y diff Git

Abrir la raíz real del repositorio. Menú Git o Palette: Repository Status,
Active File Diff, Compare HEAD/Index, Refresh/Cancel y Choose Git Executable.
La primera consulta solicita un ejecutable de confianza instalado; también se
puede indicar `cide-gui --git-executable ABS`. Solo se conserva durante la sesión,
sin instalación ni configuración global. El selector modal suelta los préstamos
del modelo y revalida raíz/documento antes de ejecutar la intención capturada.

Estado no requiere documento ni encoding. Up/Down/Page/Home/End seleccionan filas
con flags tipados index/worktree; Enter **no abre paths**. F5 refresca y Escape
cancela/cierra. La consulta representa disco/index, sin cambios de buffers sucios,
renames/submódulos o preferencias globales del Git personal.

Diff compara el archivo activo, incluidos cambios sin guardar, contra un blob
inmutable de HEAD o del índice. Se muestran OID, commit de HEAD cuando existe,
hunk @@, signos -/+ y tres líneas de contexto. F2 HEAD/F3 index cambian origen;
F5 refresca; Up/Down cambian hunk; Page y Home/End recorren su cuerpo; Enter navega
por el destino derecho validado y cierra el panel sin editar ni guardar.
Revisión, encoding, vista, ruta, cierre, raíz o ejecutable distintos invalidan
datos y navegación. No watcher ni refresh implícito por cada tecla.

GitStatusController mantiene su worker de repositorio lazy/acotado. GitDiffController
usa un JobSystem compartido de la GUI, lazy y con cuotas F3 existentes; no otro pool
por consulta ni snapshots duplicados mientras espera un trabajo anterior. Poll del
mensaje nativo es la única aceptación de completions, con recuperación de panic
incluso obsoleto. GitDiffController.validate_context separa invalidación metadata
de tick/admisión: preparar una escena no copia fuentes ni admite/acepta trabajo.

La vista escapa paths opacos/Unicode/controles/bytes inválidos,32 filas y256 columnas
ASCII como máximo. Diff lee hasta2048+4 bytes prestados por prefijo visible, con
encoding explícito, BOM y LF/CRLF/CR/EOF visibles; cuerpo virtual por segmentos,
sin materializar una línea larga o todas las filas. GDI consume la escena readonly.

Se conservan contención, entorno aislado, límites y limpieza de GitReader. Solo
lecturas de Git; stage/commit/push/pull/merge pertenecen a M5 por acción explícita.
No se promete sandbox adversarial del repositorio ni latencia dura de I/O/kernel.

## Validación

El recorrido usa únicamente HWND oculto, fixtures create_new y un repositorio sintético
en work. Python prepara HEAD/index allí; producción solo consulta. Los hashes de
todos sus archivos .git antes/después detectan escrituras de index/config/objetos/refs.
No usa clipboard OS, pantalla o documentos del propietario. Bitmap GDI no equivale
a pantalla física. IME, DPI/accesibilidad y aceptación global RAM/latencia siguen
abiertos; las pruebas de Juan permanecen al final. CI remoto dormido hasta M5.

Cierre local Windows/MSVC offline: **53 pruebas Rust distintas dirigidas**,
30 GUI,10 S1,9 entrega Git y4 TUI de proyecto. La primera selección de Cargo con
--test project_search ejecutó solo sus10; GUI se ejecutó por separado. Los25 GUI
intermedios se solapan con los30 finales y no se cuentan dos veces. Ocho casos
nuevos (3 preview,4 proyección Git,1 parser); total histórico399, sin corrida399.
Formato, Clippy workspace/all-targets y release workspace bins/examples aprobaron.
Clippy detectó format_collect en un fixture: se sustituyó por write! y pasó.

**58 controles nativos** del release aprobaron:17 nuevos (10 reemplazo y7 Git),
fixture13B intacto,1 WM_PAINT y0 errores. Se verificaron los hashes de todos los
archivos .git del repositorio sintético, incluidos index/config/objetos/refs; no
cambiaron. HEAD e índice tienen blobs distintos y las comparaciones derechas usan
el Q dirty real del Workbench. Preparar/componer GDI no admite ni consume jobs.

La primera corrida integral detectó un oráculo incorrecto: después de Undo, el ID
de una revisión nueva no tiene por qué ser el anterior+1. Se corrigió la expectativa;
un Undo exacto demuestra una única transacción. Se recompiló solo GUI y se repitió
ese recorrido integral. No se repitieron las suites Rust aprobadas ni perfiles.

La inspección GDI también ajustó el draft independiente para mostrar varias
líneas a la vez y actualizó el estado Git visible al completar. Se recompiló solo
GUI y el mismo recorrido58 volvió a pasar; los artefactos final-* corresponden a
esa versión, los del prefijo anterior conservan la primera corrida aprobada.

- [Reporte nativo58](evidence/m2b-replace-git-final-2026-11-03-native-report.json).
- [Metadata/hashes y método de conteo](evidence/m2b-replace-git-final-2026-11-03-metadata.json).
- [Resumen y hashes Git](evidence/m2b-replace-git-final-2026-11-03-summary.json).
- [Preview](evidence/m2b-replace-git-final-2026-11-03-native-project-replace.png),
  [draft multilínea](evidence/m2b-replace-git-final-2026-11-03-native-project-replace-draft.png),
  [diff](evidence/m2b-replace-git-final-2026-11-03-native-git-diff.png) y
  [status](evidence/m2b-replace-git-final-2026-11-03-native-git-status.png): raster GDI,
  convertido sin pérdida de BMP a PNG, no captura de pantalla física.
- Logs crudos `evidence/m2b-replace-git-final-2026-11-03-*.log`, incluido el fallo Clippy;
  reporte `failed-oracle.json` conserva el primer recorrido fallido.

Las seis ayudas y los recursos de icono de los seis PE se verificaron tras el
build. Sin dependencia/lockfile nueva, red, instalación, CI ni push. La GUI sigue
sin cierre de IME/DPI/accesibilidad, columnas Unicode generales o rendimiento
físico/global. Siguiente construcción M2B: Explorer lateral sobre catálogo común
acotado, conservando apertura/versiones/vistas; no otro indexador residente.
[Contrato y deuda TUI observada](M2B_EXPLORER_CONTRACT.md). El árbol GUI todavía
no está implementado; el contrato fija IDs/cuotas/cola/migración antes de ampliarlo.

Paquete local: `work/packages/CIDE-windows-dev-2026-11-03.zip`,69 entradas,
3698346 bytes, CRC y hashes internos verificados. Dos notices GNU winapi ausentes
en el cache offline se registran expresamente; no se declara revisión de distribución
ni equivalencia con el grafo enlazado MSVC. No instalación/publicación.
[Resultado y SHA256](evidence/m2b-replace-git-final-2026-11-03-package.json).
