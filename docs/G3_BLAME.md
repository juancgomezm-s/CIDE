# Anotaciones por línea — 2026-11-06

GUI y TUI consultan atribuciones de una versión confirmada a través del núcleo
Rust compartido. No se atribuye el buffer sin guardar, no se modifica el archivo
y no se ejecuta git.exe en producción.

## Contrato del núcleo

`jc_git::blame::read_blame(root, pinned_start, path, first_line, count, limits,
cancelled)` entrega una página con identidad del repositorio, commit inicial,
ruta, OID del blob y snapshot original inmutable. Cada fila tiene su número de
línea original, rango exacto de bytes en ese snapshot y un índice opcional hacia
los metadatos del commit responsable. El número de línea en la revisión responsable
se conserva por separado. Los campos de líneas son base cero; la presentación
los convierte a base uno y no los interpreta como offsets de bytes del editor.

La atribución sigue únicamente el primer padre y la misma ruta exacta. Reutiliza
`DiffModel` y sus correspondencias de tramos iguales para trasladar líneas hacia
sus antecesores; inserciones/reemplazos se atribuyen al commit correspondiente.
No hay otro algoritmo de alineamiento ni documento editable histórico.
No se siguen renombrados, copias o todos los padres de una integración. Por ello
se muestra **primer padre**, sin afirmar equivalencia con todas las opciones de
git blame o con el comportamiento completo de Sublime Merge.

Las líneas pendientes conservan `commit=None`. `BlameBoundary` distingue
atribución completa del rango solicitado, frontera shallow, límite de recorrido,
lectura o comparación. Nunca se inventa un autor al alcanzar una cuota. Objetos
anunciados ausentes/corruptos, rutas no admitidas y cancelaciones siguen siendo
errores. Un archivo presente vacío entrega cero filas; una ruta ausente es error.
El inicio queda fijado aunque HEAD avance. No se leen archivos del worktree.

## Presupuestos y entrega

Hasta 128 filas por página y 128 commits examinados. Fuentes hasta 2 MiB por blob,
4 MiB por par de alineamiento y 16 MiB de lecturas acumuladas de blobs por pedido.
El snapshot original retenido puede coexistir con el par de antecesores: hasta
6 MiB de payload de fuentes en ese momento. No es una garantía de RSS.
Se conservan cuotas de árboles de 8 MiB acumulados y lecturas de commits de
64 KiB; el presupuesto de metadatos del host es de 4 MiB. La comparación dispone
de 64 MiB de unidades de trabajo acumuladas y 200.000 registros de líneas entre
ambas fuentes. El índice inicial contra el mismo archivo admite por ello hasta
100.000 líneas de archivo. Cancelación y timeout de 5 s son cooperativos.
Autor/resumen se limitan a 96/256 bytes con indicador de recorte.

`GitRevisionController` añade esta consulta al único worker y su cola existente.
Solo poll acepta una entrega, comprobando raíz, generación, inicio, ruta, página
y límites/índices del resultado. Al abrir el par de un commit responsable se
conserva la página para regresar; cambiar consulta, raíz o cancelar la descarta.
No se acumulan todas las páginas ni se duplica undo/Document/FileSession.
Los adaptadores de OmniCore/OmniCoder no se declaran integrados por esta entrega.

## GUI

**Anotaciones** en el detalle Git, o **L** dentro del espacio Git, consulta la
ruta seleccionada. En un repositorio limpio admite el documento activo dentro
del proyecto. Desde una comparación histórica se fija a esa revisión; desde
historial por archivo puede usar la revisión seleccionada.

La tabla muestra número, commit, autor y contenido; la franja izquierda resume
la autoría de la página. Las líneas pendientes se identifican sin atribución y
la cabecera/pie muestran el motivo. **Anterior/Siguiente**, N/P o Re/Av Pág
consultan otra página; rueda y barras desplazan su porción visible. E o el control
de codificación elige la interpretación sin cambiar bytes. Se reutiliza el
proyector acotado de texto Unicode/TAB/bytes inválidos y sus avisos de fila limitada.

Clic en la atribución o Intro abre el diff del commit responsable. **Volver a
anotaciones** o Esc/B regresa a la misma página, recuperando viewport y foco.
El diff histórico nunca se usa para saltar a una posición del buffer vivo.
R/F5 relee la página fijada; Esc/B desde anotaciones vuelve al estado Git.
Se conserva el tema oscuro, la geometría nativa y los hits versionados; no hay
lecturas de repositorio ni aceptación de resultados durante preparación/pintura.

## TUI

L desde Estado Git/historial abre las anotaciones. El lateral lista autor y
commit; la vista inferior muestra prefijos de fuente del snapshot confirmado.
N/P y Re/Av Pág paginan, E cambia codec, Intro abre el par responsable y Esc/B
regresa. H retorna al historial general. Si una página histórica previa se retiró
al abrir otro commit, el regreso la solicita de nuevo desde el handler.
Los errores y atribuciones pendientes se presentan explícitamente en español.

## Validación dirigida

Once casos nuevos aprobaron por suites: cinco del núcleo, dos de entrega, dos
de GUI y dos de TUI. Cubren inserción/borrado, líneas repetidas, CRLF/bytes
inválidos, primer padre de merge, raíz/archivo vacío/ausencia, HEAD posterior,
paginación, fronteras shallow/recorrido/lectura/comparación, cancelación y objetos
ausentes. Se verificó además que solo poll acepta y que un rango inválido o una
raíz cambiada no publiquen datos. El cierre de cuota repitió únicamente el caso
afectado después de ajustar el límite de lectura de commits.

GUI recorre 130 líneas en páginas 128+2; TUI recorre 142 en 128+14. Abren la
revisión responsable, regresan, cambian interpretación y conservan el buffer
sucio, vistas y fuentes originales. Se comprueba la caducidad de hits y que las
teclas no activen pares locales detrás de las anotaciones. Se corrigió un error
de compilación inicial de TUI al reiniciar un struct con un campo privado;
el log se conserva sin contarlo como prueba aprobada.

Formato, Clippy selectivo all-targets y release offline se registran en el JSON
del bloque. No se repitieron suites completas, pruebas de escritorio, HWND,
ConPTY, CI ni push. No se usó recuperación o portapapeles personal. IME sigue
excluido, Narrador aplazado y las puertas físicas/globales de M2 no se amplían.

Evidencia: `docs/evidence/g3-blame-2026-11-06.json` y logs `work/blame-*.log`.
