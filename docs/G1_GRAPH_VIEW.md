# Grafo de commits en GUI y TUI

Abre **Git** en la GUI o **Estado Git** en la TUI y pulsa **G**. En la GUI
también puedes elegir **Grafo**. Los carriles muestran la ascendencia del
HEAD observado. **◆** marca una integración y **○** marca un corte de
ascendencia. La fila muestra el SHA abreviado, el resumen y el autor.

Usa las flechas y **Intro** para abrir el commit seleccionado. En la GUI
también puedes pulsar la fila. Desde los archivos del commit, **Esc**
regresa a la misma página y selección. En la TUI, si estás en el diff,
**Esc** devuelve primero el foco a la lista de archivos. **N** muestra
la siguiente página. **F5** vuelve a consultar HEAD.

El visor conserva hasta 512 commits de la ascendencia de HEAD y los
presenta en páginas de 24, con cada hijo antes de sus padres admitidos.
Los carriles continúan al pasar de página. La cabecera distingue el
límite de ascendencia, una copia shallow y las abreviaciones de texto o
carriles. La ventana se limita a las ramas alcanzables desde HEAD.
El detalle de un merge compara con su primer padre.

El núcleo conserva una observación fijada de la ascendencia. La GUI y la
TUI usan el trabajador de revisiones existente. Al abrir un commit, la
página queda disponible para volver sin otra consulta. La lectura y la
presentación del grafo se hacen fuera del editor y conservan las fuentes.

Las pruebas dirigidas comprobaron la bifurcación y unión de carriles,
el corte por cuota, la navegación a un merge en la segunda página y el
regreso a la selección. La prueba del núcleo distinguió también una
frontera shallow de un objeto faltante.

Formato, Clippy y compilación optimizada offline aprobados. La GUI pasó
26 comprobaciones con ventana oculta, incluido el pintado del grafo y
el regreso desde el detalle. La TUI optimizada pasó el recorrido del
grafo en un ConPTY privado, además de preparar un bloque y modificar el
último commit. CIDE se ejecutó con PATH vacío en ambas pruebas.
Entrega en `target/git-graph-next/release/`.
