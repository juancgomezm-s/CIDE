# Propuesta visual Git — punto seguro del 6 de noviembre de 2026

Juan pidió rediseñar primero la presentación siguiendo sus capturas de Sublime
Merge, con el tema oscuro de CIDE y todo en español. Mostrar el resultado antes
de implementar o agregar funcionalidades. Después pidió pausar para reiniciar.

## Entregables para revisar al retomar

- `cide-git-historial.png`: composición de ubicaciones, historial, archivos y diff.
- `cide-git-cambios.png`: misma composición con cambios locales seleccionados.
- `render.py`: generador reproducible de ambas imágenes con Pillow existente.

Son maquetas estáticas con datos ilustrativos, NO capturas de una GUI implementada.
Los controles y categorías dibujados no demuestran capacidades disponibles.
No se cambió la aplicación, el motor Git, recuperación, dependencias ni binarios.
Se revisaron visualmente ambas composiciones; no corresponde ejecutar Cargo para
este bloque de diseño. La última implementación permanece en el commit e0e163a.

## Dirección visual propuesta

Lienzo de 1680 × 960. Fondo de comparación #2E3440; paneles #272D37/#2B313C,
cabeceras #353D4A, selección #414E62. Verde y rojo discretos para diferencias.
Tipografía Segoe UI uniforme y compacta; Consolas para el código. Iconos simples
dibujados para evitar caracteres ausentes de la fuente.

Ubicaciones 185 px, historial 325 px, archivos 230 px y comparación 940 px.
Una sola barra de herramientas de 42 px, menús de 30 px y estado de 24 px.
Controles pequeños, divisores discretos, selección clara, nombres y rutas en dos
niveles y resumen breve del commit sobre el diff. Sin tarjetas grandes de acciones
ni explicaciones de cuotas, workers o estados internos en la interfaz cotidiana.

## Próximo paso

Al recibir el aviso de Juan, mostrar ambas imágenes para revisión del diseño.
No seguir con historial por archivo ni implementar esta presentación antes de esa
revisión. La propuesta aún puede necesitar ajustes solicitados por Juan.
Después trasladar la composición aprobada al pintor nativo, conservando el núcleo
independiente, los límites de proyección y la aceptación de resultados fuera de
render. No confundir validación funcional previa con aceptación visual.

El reinicio no implica que haya una ventana de prueba viva o cerrada. Si se requiere
escritorio, seleccionar ventanas de nuevo con Computer Use. IME excluido, Narrador
aplazado y segundo monitor sin verificar permanecen como estaban.
