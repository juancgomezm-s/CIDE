# Marco, pestañas y paneles de CIDE

El bloque reúne las observaciones de Juan sobre navegación entre documentos,
redimensionado, controles inferiores y exceso de entradas en los menús.

## Presentación y rutas

- Las flechas de desplazamiento de pestañas quedan juntas a la izquierda.
  El botón de documento nuevo y la lista filtrable de documentos abiertos quedan
  a la derecha. Las pestañas inactivas tienen delimitadores verticales. La lista
  sigue seleccionando por identidad, con títulos duplicados y miles de documentos.
- El Explorador conserva el lado izquierdo. Código ocupa un panel independiente
  a la derecha, con símbolos y consultas explícitas Llama a, Llamado por y
  Dependencias. Los controles dependen de capacidades: JavaScript dispone de
  símbolos/importaciones y referencias entrantes, pero no de llamadas a funciones.
  ABL conserva RUN, inclusiones y PROPATH mediante su proveedor opcional.
- Intro abre el resultado y Ctrl+Intro lo abre al lado. Las referencias ambiguas
  presentan candidatos dentro del mismo panel. Se conservan identidad, revisión,
  codificación, confinamiento y verificación de disco al navegar.
- Solo símbolos se actualiza automáticamente al cambiar el documento. Las consultas
  avanzadas se piden explícitamente; un cambio de contexto oculta sus resultados
  y el botón permite consultar de nuevo. No se crea un indexador permanente.
- La barra inferior permite ocultar/mostrar Archivos y Código por separado.
  Conserva posición, codificación y estado; resume lenguaje y cantidad de cambios
  del último estado Git aceptado. No inventa una rama ni ejecuta Git al dibujar.
  La consulta Git sigue siendo explícita y opcional.
- En poco ancho se conserva el documento. Código se omite temporalmente cuando
  no cabe, especialmente si impediría mostrar ambos grupos. La preferencia de
  visibilidad se mantiene y vuelve al ampliar la ventana.

## Redimensionado

Los ocho bordes usan dimensiones actuales del HWND y una zona de adquisición de
8 píxeles de diseño, escalada por DPI. La prueba de escritorio reprodujo que
arrastrar la esquina no cambiaba el tamaño. La limpieza común de captura llamaba
a ReleaseCapture después de WM_SIZE cuando no había un arrastre de editor,
interrumpiendo también la captura nativa de Windows. Ahora el propietario registra
solo las capturas adquiridas por el editor y la limpieza libera únicamente esas.
Windows sigue siendo dueño de mover/redimensionar; no se añade un bucle paralelo.

## Compatibilidad de sesión

Los metadatos gráficos versión 2 añaden la visibilidad del panel Código. El lector
acepta también versión 1, con Código visible por defecto. Los bytes de documentos,
deshacer, revisiones y referencias siguen usando los contratos de recuperación
existentes. No se escribe el original ni se modifica el depósito personal en pruebas.

[Auditoría de menús](M2_MENU_AUDIT.md). El inventario distingue flujos existentes
de AVM, base de datos, terminal y ayuda aún pendientes, que no se anuncian como
herramientas funcionales en los menús visibles.

## Validación

La evidencia automatizada y la comprobación final de escritorio se registran en
[VALIDACION](VALIDACION.md) y [M2_CIERRE](M2_CIERRE.md). Los informes intermedios
fallidos se conservan en `work/m2-panels-*.log`; no equivalen a una corrida global
aprobada. Siguen vigentes las puertas físicas de M2 y los servicios semánticos
M3–M4; este panel consume capacidades léxicas existentes.
