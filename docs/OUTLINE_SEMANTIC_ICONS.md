# Iconos semánticos de Esquema

Los grupos de código dejan de utilizar el icono de carpeta. El árbol distingue
explícitamente los directorios reales de las categorías de símbolos y los
espacios de nombres. La navegación y el orden de los elementos se conservan.

Los pictogramas son vectores GDI originales de 20 × 18 píxeles lógicos:
clases y objetos con cubo tridimensional hueco, métodos con engranaje,
funciones con ƒ(x), procedimientos con documento y ejecución, tablas temporales
con rejilla y reloj, inclusiones con hojas y <>. Variables y constantes tiene
un pictograma de valores propio; cada variable conserva su icono de tipo.
También hay símbolos para parámetros, búferes, módulos en capas, prototipos,
índices, campos, consultas y relaciones de llamadas.

Las categorías usan la misma clasificación de símbolos para todos los lenguajes;
no dependen de que el archivo sea ABL. Solo los segmentos de rutas de directorios
conservan carpetas; los prefijos de espacios de nombres usan su pictograma.
No se incorporaron paquetes externos, fuentes de emoji ni lecturas de disco
al pintado. Los colores se adaptan a las preferencias y a la selección.

## Validación local Windows

17 pruebas dirigidas de Esquema aprobadas; una de un perfil OpenEdge real quedó
omitida por requisito de entorno. Los 25 pictogramas se dibujaron en 450
combinaciones de DPI (96/144/192), tema (actual/claro/alto contraste) y fondo
(normal/selección). Se comprobaron diferencias de trazos/rellenos sin depender
del color, los límites de cada icono y la restauración del recorte del DC.
Las pruebas existentes de miniiconos de tipos también pasaron.
Fmt, Clippy GUI all-targets y compilación debug aprobaron.

La galería work/semantic-outline-icons/iconos-nativos-final.png procede del
mismo dibujante nativo utilizado en el panel. Es una composición GDI en memoria,
no una captura física de escritorio. No se utilizó el escritorio en este bloque.

Este cambio es posterior al MSI 0.1.12 publicado. La entrega local de desarrollo
se identifica como 0.1.13-dev; no se ha publicado un nuevo MSI en este bloque.
