# Consulta histórica en el núcleo Git compartido

Cierre local: 6 de noviembre de 2026. Incremento básico de consulta para M2;
no cierra toda la expansión G1–G3 ni habilita escrituras de M5.

## Implementación

`jc-git::revision::read_revision` enumera los archivos cambiados entre un commit
explícito y su primer padre. Conserva el OID del commit y del padre; en un commit
raíz el lado anterior no existe. Las fusiones indican sus padres y que se compara
el primero. No hay detección de renombrados ni elección de otro padre en esta UI.

`read_file_pair` entrega bytes inmutables de ambos lados con OID/modo, distinguiendo
archivo presente vacío de entrada ausente. Solo una ausencia comprobada de entrada
de árbol produce None: corrupción, objeto anunciado inexistente, tipo no admitido
o ruta que atraviesa symlink/gitlink siguen siendo errores. La lista verifica
commits/árboles; los blobs se verifican al pedir el par, no se precargan al listar.
La lectura queda fijada al commit solicitado aunque HEAD cambie después.

El lector admite OIDs hexadecimales completos, no revspecs de consola. Conserva
rutas Git opacas. Límites del adaptador: 2 MiB por fuente, 4 MiB combinados y de
metadatos, 4.096 archivos, ruta hasta 4.096 bytes y timeout cooperativo de 5 s.
El core limita además profundidad a 128, visitas de entradas a 65.536, lectura
acumulada de árboles a 8 MiB y padres retenidos a ocho. Exceder la lista/cuotas
rechaza la consulta; no presenta archivos truncados como una lista completa.
Un exceso de padres se marca limitado y conserva el primer padre explícito.
Estas cuotas lógicas no son una garantía de RSS o latencia física.

## Independencia y adaptación

El único modelo de alineamiento se trasladó de jc-app a
`jc-git::diff_model`. Solo usa std, no el editor ni la UI. La ruta anterior
`jc_app::diff_model` reexporta ese mismo modelo para conservar a los consumidores;
no conserva una segunda implementación. Consultas históricas y comparación están
disponibles desde la API pública del núcleo, conforme a [D63](GIT_SHARED_CORE.md).

`jc-app::git_revision` coordina el host: un worker perezoso, un pedido activo,
un pendiente de metadatos y una entrega que retiene la ranura hasta poll. La
generación y raíz se comprueban antes de aceptar; cancelar o cambiar contexto
invalida entregas anteriores. Al pedir un archivo del mismo commit se conserva
la lista y se retira inmediatamente la comparación anterior. El worker mueve
los bytes al modelo, sin clonar Document/FileSession/undo. Drop cancela y une
su thread; las tareas se aceptan únicamente en las rutas de poll existentes.

El consumidor externo de integración importa solamente `jc_git::revision` y
`jc_git::diff_model`, prepara su repositorio con gix y no usa git.exe. Esto prueba
el consumo independiente de CIDE; no declara integrados OmniCore ni OmniCoder,
ni define todavía una ABI estable para otros lenguajes.

## Flujos GUI y TUI

GUI: clic en un commit del historial carga su lista de archivos. Clic/Intro en
un archivo prepara la comparación; Resumen muestra commit y primer padre. Cambios
locales regresa al estado existente. Tab cambia lista/comparación; F5 actualiza
la consulta fijada; Esc vuelve al editor. Los bloques y prefijos se componen de
capturas aceptadas, con hits versionados. El historial tiene viewport independiente
de la selección. El encabezado de archivos usa dos líneas para evitar recorte.

La codificación histórica es una elección separada del buffer. Se admite el
valor explícito de lanzamiento; de otro modo se pide elegirla. El botón recorre
UTF-8, Windows-1252, ISO-8859-1 e IBM850 sin convertir ni guardar los bytes.
Los prefijos inválidos se muestran escapados. No se infiere el codec a partir
de un documento ajeno que casualmente esté activo.

TUI: desde el panel Estado Git, H muestra Historial; Intro elige commit y luego
archivo. E selecciona interpretación, las flechas desplazan y ←/→ cambian bloque
en la comparación. Esc vuelve de comparación a archivos y luego a commits; H
regresa al estado local. Las ayudas se muestran en la vista. Ratón y teclado
usan destinos de consulta; una revisión histórica no navega al buffer editable.

El árbol histórico no se abre todavía como pestaña lateral inmutable. Tampoco
hay DAG completo, todas las referencias, búsqueda histórica ni anotaciones por
línea. Las comparaciones preparadas/disco/buffer actuales conservan su contrato
previo: HEAD o índice frente al buffer. Corregir sus pares explícitos sigue como
siguiente bloque de cierre M2; esta entrega no los presenta como implementados.

## Validación y límites

Se aprobaron 734 pruebas en una corrida del workspace antes del traslado final
del modelo. Después se aprobaron 335 casos de core/adaptador por ese traslado,
un flujo adicional de teclado histórico TUI y un consumidor externo del núcleo.
Son 736 casos distintos cubiertos por corrida y cierre dirigido, no una corrida
integral final de 736. Formato, Clippy workspace/todos los destinos con warnings
denegados y release offline aprobaron. No hay dependencia nueva ni cambios al lock.

La GUI aprobó 142 checks sobre su HWND propio: incluye la revisión raíz, ausencia
del lado anterior, fuente histórica fijada y preservación del buffer sucio/vistas.
El harness comprobó que índice, refs, objetos y fuentes de su repositorio quedaron
intactos, con PATH vacío durante la ejecución de CIDE. Siete checks ConPTY del
explorador/mouse pasaron con el ejecutable nuevo. El flujo H/Intro/E/Esc se probó
por el dispatcher de teclado TUI con un repositorio real de fixture y buffer sucio.

La revisión visible no pudo completarse: activar la ventana devolvió GetCursorPos
acceso denegado (0x80070005); una nueva selección/captura resultó negra. No se
inyectó entrada a ciegas ni se forzó cierre. Queda una instancia propia sin cambios
para salida normal cuando el escritorio esté disponible. Renovar selección con
Computer Use, nunca actuar sobre un PID/handle antiguo. Launch y limitación:
`work/g1-history-{visual-launch,desktop-result}.json`. Se verificó que el fixture
visible, incluidas sus fuentes y metadatos Git, no cambió. No se usó depósito personal.

Ejecutables vigentes: `target/m2-history-next/release`; anteriores preservados.
Evidencia resumida: [g1-historical-reads-2026-11-06.json](evidence/g1-historical-reads-2026-11-06.json).
IME real excluido por Juan; Narrador aplazado, segundo monitor no disponible y
aceptación global de pantalla/RAM pendientes. No se ejecutó CI ni se hizo push.
