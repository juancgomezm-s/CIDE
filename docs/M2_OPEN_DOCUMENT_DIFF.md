# Comparación de documentos abiertos — TUI, 2026-11-03

Integrado sobre f7043e0. Se recuperó el patch del checkpoint66aa783, aplicado
por contexto y adaptado en lib.rs; no se sustituyeron fuentes actuales por
copias antiguas. Resaltado y regex se conservan. El panel GUI del checkpoint
permanece archivado, sin módulo activo ni afirmación de paridad gráfica.

## Uso

Mostrar documentos distintos en Group1 y Group2. View o Command Palette:
**Compare Open Groups (Read-Only)**. No requiere carpeta, Git o archivo guardado;
compara los bytes actuales, incluidos cambios sin guardar. El título identifica
ruta, encoding y dirty de cada lado. No convierte fuentes entre encodings para
decidir igualdad: son diferencias de bytes, con interpretación explícita al mostrar.

Up/Down seleccionan hunk; PgUp/PgDn recorren su cuerpo; Home/End primero/último.
Enter navega al grupo derecho original; Shift+Enter al izquierdo. Los comandos
Open Comparison Hunk in Left/Right Group son alternativas de paleta accesibles
por teclado. F5 refresca; Escape cancela y oculta. Caracteres/pegado/Delete no
editan fuente mientras se muestra la comparación. Menús y paleta siguen disponibles.
Navegar respeta la identidad original incluso con pin: no sustituye la referencia
del otro grupo. Cerrar/editar/cambiar encoding/ruta/vista invalida el resultado.

Desde92 columnas terminales usa filas laterales L|R; con menos ancho usa filas
unificadas -/+. Es un visor de snapshots de solo lectura, no un tercer editor ni
sincronización automática del scroll de editores. No hay aplicación de hunks,
merge ni escritura Git. Tres líneas de contexto por extremo del hunk seleccionado.
No byte differences significa igualdad de bytes, aunque las interpretaciones difieran.

## Arquitectura y límites

OpenDiffController conserva una solicitud en vuelo y solo metadata pendiente
de la más reciente. El JobSystem F3 admite ambos snapshots juntos después de
validar revisiones, encodings, rutas y ViewIds. Máximo2MiB por lado; cuota conjunta
de snapshots y presión global F3 antes de copiar. No clona Document, FileSession
ni undo. Ambos snapshots usan Arc y OwnedDiffModel conserva esas mismas fuentes
inmutables, sin otra copia completa. Workers no reciben Workbench.

El artefacto reserva12MiB; metadatos del modelo hasta4MiB y fuentes hasta4MiB,
con cuotas de trabajo/líneas/hunks propias de DiffLimits. Deadline10s cooperativo.
La reserva de artefactos en vuelo es global F3; el resultado aceptado pertenece
al controller hasta invalidación/cancelación/reemplazo. No es garantía de RAM
privada global, ni suma de todos los controllers aceptados del proceso.

Entrega comprueba **ambos** orígenes y generación. Cancelar o refrescar marca
la ejecución anterior para descarte y espera su acknowledgment antes de otra
captura. La misma cola/poll de F3 entrega los resultados; no nuevo worker/indexador
ni poll desde draw. Preparar puede invalidar metadata; no admite snapshots ni
acepta completions. Se omite preparar/revelar los editores ocultos por este visor.

OpenDiffView compartido prepara hasta96 filas,256 columnas y prefijos de2048
bytes por línea. Solo el hunk/contexto visibles. Escapa controles/invalid bytes,
representa BOM inicial y EOL exacto; cada lado usa su encoding. La TUI compone
esas filas sin leer el modelo ni cambiar vistas. Estado/pin/historial y undo
siguen perteneciendo al Workbench. No reemplaza el visor Git existente.

## Validación

54 casos distintos observados aprobados:5 nuevos controller/presentación,
2 nuevos TUI,8 F3/jobs,9 GitDiff,15 interacción,9 panes y6 matriz de teclado.
Los logs contienen40 nombres distintos; un caso TUI repetido está incluido en
esos40, los restantes14 se observaron en las invocaciones dirigidas previas.
No suite workspace completa ni perfiles globales repetidos. [Metadatos y hashes](evidence/m2-open-diff-2026-11-03-metadata.json).

Fmt, Clippy workspace/all-targets y release workspace bins/examples aprobados.
Tras corregir estado de Escape, solo se repitió su caso TUI y Clippy/build TUI.
La primera regresión Git falló al limpiar un fixture por acceso denegado y
envenenó el mutex de los demás casos; la suite de nueve pasó al ejecutar con
acceso autorizado a sus repositorios temporales. Ninguna operación sobre el
repositorio del usuario fue parte de esas pruebas.

Fallos iniciales conservados: nombre equivocado de método en fixture, prefijo
unificado que excedía una columna, lint format_collect; oráculos ConPTY de
capitalización/foco y mensaje de cancelación obsoleto corregido en producto.
[ConPTY final](evidence/m2-open-diff-2026-11-03-conpty-3.json): cuatro flujos,
46705 bytes VT, salida0 y dos fixtures intactos; entrega sin otra tecla/Git,
vista compacta/lateral, entrada de solo lectura y navegación a ambos grupos.

[Paquete local verificado](evidence/m2-open-diff-2026-11-03-package.json):
`work/packages/CIDE-windows-dev-open-diff-2026-11-03.zip`,5398311 bytes,
76 miembros, CRC/hashes correctos. SHA256
`3a1ffd4243ea153d92623da8362c8b0048319d8e16cf28e01f64da5f29ee5874`.
Continúan dos avisos GNU pendientes, uso de desarrollo. Sin publicación/CI,
aceptación física/global, clipboard OS ni implementación GUI de este visor.

El siguiente bloque TUI es el contrato de capacidades internas y su consumidor
de símbolos/navegación local; no inferir AVM, completado semántico o parser
certificado a partir del resaltado. Ver [pendientes TUI](M2_TUI_FINISH.md).
