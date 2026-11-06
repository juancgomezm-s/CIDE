# Espacio Git nativo y selector con tema — 2026-11-05

La referencia de organización es la captura de Sublime Merge aportada por Juan.
CIDE conserva el tema oscuro elegido (#2E3440), su fuente y sus controles nativos.
Este bloque sustituye la presentación Git basada en el panel de búsqueda.

## Interfaz integrada

La vista ocupa el área de contenido, con encabezado de repositorio/rama actual,
historial local, archivos cambiados y comparación. A partir de 760 píxeles de
diseño muestra tres columnas; en ventanas menores prioriza archivos/comparación.
Actualizar y Volver al código son acciones reales. La lista usa estados tipados
para distinguir preparado, cambios de trabajo, no rastreados y conflictos.
Seleccionar un archivo reutiliza el documento abierto, incluidos sus cambios sin
guardar, y compara con HEAD o índice. Flechas, Enter, Tab, rueda y controles de
bloque permiten navegar. El texto original y los metadatos Git permanecen intactos.
Las rutas opacas se presentan escapadas; solo una ruta relativa válida puede abrirse,
a través del lector confinado de Workbench. Un archivo eliminado o sin objeto base
puede rechazar la comparación; se informa el motivo en lugar de inventar contenido.

El historial muestra hasta 24 commits por primer padre y detalles reales de autor,
identificador y número de padres. No es un grafo completo de todas las ramas ni una
comparación histórica de archivos. No hay botones simulados de commit, push o merge:
las escrituras/red pertenecen a M5. Próximo incremento de consulta: selección de
revisiones y archivos históricos, previa identidad/versionado y cuotas de objetos;
seguido por navegación de referencias. No activar mutaciones para imitar la captura.

El proveedor Rust fija HEAD una sola vez para estado y metadatos. Admite hasta ocho
padres por commit, resumen de 256 bytes y autor de 96; carga hasta 24 KiB de metadatos
a la cuota total antes de admitir filas. La captura indica recorte/historia parcial.
Una carpeta normal sin .git produce NotRepository y un estado sin repositorio,
sin modal de error. Un .git presente pero inválido sigue siendo error.

Pintar solo consume capturas aceptadas. Las tareas se aceptan por la notificación
existente; no se hacen consultas al pintar. Los clics se verifican contra la época,
raíz, geometría, revisión y codificación de su cuadro publicado. Se retiró la
proyección Git anterior y su segundo estado de desplazamiento.

## Selector de apertura

Abrir carpeta y Abrir archivo usan un selector propio con encabezado, fondo y
botones del tema, ruta editable, [..], Subir y lista con barra/rueda. Se inicia en
el proyecto actual; no se conserva ningún préstamo del modelo durante el modal.
Usa el catálogo asíncrono existente y listas acotadas, sin leer directorios al pintar.
Tamaño inicial 640 × 420 de diseño, limitado al monitor; escala física de filas
redondeada por DPI. Guardar como conserva su flujo anterior de confirmación.

## Evidencia local

[Resumen verificable](evidence/m2-git-workspace-2026-11-05.json).
718 casos aprobados mediante suites y cierre dirigido, no una sola corrida integral.
La primera corrida detectó una expectativa de redondeo DPI incorrecta (375/38 da
nueve filas completas); se corrigió la prueba y pasó el cierre. La prueba nativa
integral aprobó 139 checks, con PATH vacío y metadatos Git sin cambios.
Se aisló el fixture inferior de tres regiones del panel Código dejado visible por
pruebas anteriores; la navegación real de cuatro regiones conserva su contrato.
Formato, Clippy todos los destinos y release offline aprobaron.

En escritorio se observó el selector con su tema, cancelación, apertura de Git desde
la barra, comparación mediante clic, detalles del commit y salida normal. El archivo
sintético quedó intacto; no se usaron depósito personal ni portapapeles.
Ejecutables actuales: target/m2-git-ui-next/release; anteriores preservados.
IME real queda excluido por Juan, Narrador aplazado y traslado físico entre monitores
sin verificar. La revisión visible no certifica latencia/RAM globales. CI no se ejecutó.
