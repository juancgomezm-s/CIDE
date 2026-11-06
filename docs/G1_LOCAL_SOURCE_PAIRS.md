# Pares locales Git: HEAD, índice y disco

Integración local — 2026-11-06. Amplía el núcleo independiente
[compartido por los hosts](GIT_SHARED_CORE.md) y la [consulta histórica](G1_HISTORICAL_READS.md).
La validación de cierre se registra al final.

## Fuentes y propiedad

- Preparados compara HEAD con el índice capturado, nunca con un documento editable.
- En disco compara el índice con los bytes del archivo en disco, nunca con el buffer.
- HEAD → buffer e índice → buffer mantienen sus rutas anteriores y se identifican
  como tales; los cambios sin guardar no se confunden con los preparados.
- Las fuentes conservan presencia/ausencia, identidad, OID cuando corresponde,
  modo y bytes originales. Un archivo vacío presente no equivale a ausencia.
  Objeto anunciado ausente o corrupto produce error, no una eliminación inventada.
- El disco se compara sin normalización de finales de línea ni filtros Git.
  Por ello puede mostrar diferencias de bytes que el estado normalizado de Git omite.

## Lectura compartida y límites

`jc-fs` contiene primitivas de lectura confinada basadas en std y las API del
sistema. `jc-files` conserva sus exports compatibles, mientras `jc-git` consume
la misma implementación sin depender de Document, Workbench, jc-core o jc-text.
No hay una dependencia nueva descargable ni una segunda implementación Git.

El índice se captura en un Vec acotado y se verifica y analiza ese mismo Vec.
Su token SHA-256 describe los bytes efectivamente analizados, incluido el tráiler.
La suma de comprobación propia del índice se verifica cuando está presente;
el formato con suma omitida mantiene un token completo de la captura.
El parser sigue siendo gix, con un único hilo y cuota de asignación explícita.
Los formatos sparse/split, intent-to-add y los conflictos no se interpretan como
una entrada normal. Las fuentes no compatibles se rechazan explícitamente.

La consulta vuelve a comprobar sus entradas mutables antes de devolver el par.
Esto fija una observación de fuentes inmutables; no promete una transacción
atómica universal del sistema de archivos ni vigilancia permanente después de
la entrega. F5 toma otra captura. Una fila del estado anterior no garantiza
que sus archivos sigan en esa misma versión al pulsar comparar.

## Entrega y presentación

El controlador existente sirve consultas históricas y locales con una sola cola:
una operación activa, una solicitud pendiente de metadatos y una entrega.
Cambiar tipo, ruta o raíz cancela e invalida la comparación anterior. Solo poll
acepta resultados; pintar no hace lecturas ni admite tareas. Las fuentes pasan
al único modelo de comparación sin crear Document ni clonar el historial.

La codificación de estas vistas se elige explícitamente y solo afecta a su
interpretación. Navegar por sus bloques no utiliza coordenadas para modificar
un buffer potencialmente diferente. Las operaciones de escritura y red siguen
reservadas a M5; este bloque no prepara ni retira contenido del índice.

## Validación local

744 nombres de pruebas distintos aprobaron por suites y cierre dirigido, ocho
nuevos. No fue una sola corrida integral final: el intento workspace se detuvo
en una fixture I2A que omitía EXTENDED; después se corrigió también su colisión
de nombres concurrentes. El consumidor independiente final aprobó dos casos.
Las suites restantes descubrieron un mensaje TUI que truncaba su política;
los atajos se trasladaron a la barra inferior y los 13 casos Git afectados,
los cinco de entrada y los siete del controlador aprobaron al cierre.
Los logs iniciales se conservan; no se presentan como corridas limpias.

Las cuatro pruebas nuevas de jc-fs cubren bytes/identidad, vacío/ausencia,
cuotas/cancelación/rutas y exclusión de escritura por hardlink y renombrado del
ancestro durante una captura. No se añadió una fixture real de junction/symlink;
la exclusión reparse usa los atributos de los handles sin declarar esa prueba.

El consumidor público prepara todo con Rust/gix y no requiere editor ni git.exe.
El flujo TUI usa una fixture preparada con Git, pero las consultas y comparación
se ejecutan en el motor embebido. Tres fuentes distintas y un buffer sucio
comprueban que los pares no se confundan y que se preserven bytes y revisión.
Los snapshots de repositorio verifican que las consultas no escriben metadatos.

Clippy workspace/todos los destinos aprobó offline. Solo se añadió jc-fs como
paquete interno de Cargo.lock, sin descargar ni cambiar paquetes externos.

La observación de escritorio de este bloque volvió a dar una captura negra.
No se envió entrada ni se lanzó otra ventana visible. La instancia propia de
m2-history-next sigue pendiente de salida normal. No equivale a aceptación física.
IME continúa excluido, Narrador aplazado y traslado entre monitores sin verificar.

Formato y release workspace offline aprobaron. Los seis ejecutables están en
`target/m2-git-pairs-next/release`, conservando las rutas anteriores.
144 comprobaciones nativas HWND aprobaron con PATH vacío. Incluyen teclado de
pares locales, fuentes HEAD/índice/disco, buffer sucio intacto, dibujo GDI de
ambas vistas y ausencia de navegación de bloques hacia el documento vivo.
La imagen del repositorio de prueba permaneció intacta. Las capturas de los
pares se revisaron para comprobar que los controles no se solapan.

Informes: `work/g1-local-pairs-test-summary.json`, `g1-local-pairs-*.log`,
`work/g1-local-pairs-native-summary.json` y `work/g1-local-pairs-desktop.json`.
Los siete checks ConPTY anteriores siguen como evidencia histórica; no se
repitieron ni se atribuyen nuevas comprobaciones de consola a este bloque.

## Cierre pendiente

La revisión interactiva final y la salida normal de la ventana anterior siguen
condicionadas a un escritorio que permita entrada y captura. No se fuerza el
cierre ni se anuncian pruebas de pantalla física a partir de un bitmap GDI.
La expansión de layout, grafo completo y utilidades Git sigue en G2/G3 y no se
convierte en un requisito nuevo para terminar la aceptación M2 ya acordada.

El paquete local verificado es `work/packages/CIDE-M2-pares-git-20261106.zip`:
356 entradas, 355 archivos del manifiesto, CRC y todos los SHA-256 comprobados.
SHA-256 del ZIP: `2769717dd5c5306c8e8101724ac699b6e21112456124d1a187034d9eb8b93285`.
El inventario de avisos conserva los textos exactos y registra la comparación
del lock: todos los paquetes registry y sus avisos permanecen idénticos; solo
se añade el crate interno jc-fs. El primer empaquetado se negó correctamente
por el hash anterior del lock, antes de crear el ZIP.
Evidencia resumida: [JSON](evidence/g1-local-source-pairs-2026-11-06.json).
