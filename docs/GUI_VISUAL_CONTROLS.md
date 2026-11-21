# Controles visuales de la GUI

## Primer bloque: apariencia, selectores e iconos

Preferencias de apariencia usa tarjetas con seis muestras de tema,
botones de tamaños de interfaz y código, controles de minimapa y posición
de Esquema. La vista previa permite combinar ajustes sin publicar las
preferencias hasta Aplicar o Intro. Cancelar y Esc restituyen el conjunto
original. La recuperación sigue capturando el estado confirmado.

La geometría de dibujo y clic es compartida: los 27 controles quedan
dentro del panel y no se solapan en las pruebas de 96, 144 y 192 DPI.
En ventanas pequeñas se conserva el selector compacto como alternativa.
Los selectores comunes dejan de mostrar el indicador de consola > y usan
superficies, búsqueda y selección con esquinas redondeadas.

Los iconos de Archivos, Esquema y las carpetas de OmniCore usan vectores
escalados con DPI: cubiertas, pestaña, papel, pliegues y sombreado.
Los archivos tienen colores por extensión. No dependen de fuentes de emoji,
paquetes de iconos externos ni lecturas del disco durante el pintado.
Este bloque no declara rediseñados todos los formularios especializados.

## Validación local — 21 de noviembre de 2026

La suite de biblioteca GUI aprobó 211 casos; un caso que comprobaba el
antiguo prefijo > se actualizó al nuevo campo de búsqueda y se revalidó.
Son 212 casos distintos con 15 omisiones de entorno, no una ejecución
completa final de 212. La geometría nueva y Clippy all-targets pasaron.
Compilación debug aprobada. Pasaron 16 comprobaciones nativas del selector,
incluidos tema de toda la ventana, múltiples ajustes, Cancelar, Aplicar e
Intro; y 17 del shell. Usaron ventanas propias ocultas y fixtures intactos.

Se abrió la GUI real mediante Computer Use. Se verificaron los iconos,
el menú Ver y Preferencias, tema Azul profundo sobre toda la ventana,
código de 11 px, minimapa oculto, conservación de ajustes combinados y
Cancelar con retorno a CIDE, 15 px y minimapa visible. Se eligieron 12 px
y Aplicar cerró el panel conservando el tamaño y mostrando el resultado
en la barra de estado. Se restauraron los 15 px originales mediante Aplicar al terminar.

La entrega anterior 0.1.11 ya está publicada, sin estos ajustes visuales.
El bloque visual se prepara como 0.1.12 para conservar ese MSI inmutable.
Ctrl+clic sigue pendiente de comprobación física: el Teclado en pantalla
está a mayor nivel de integridad que el control de escritorio, incluso
con la autorización de Juan. No se cambió esa configuración ni se eludió
la restricción. Doble clic y Ctrl+Intro sí pasaron en siete lenguajes.
