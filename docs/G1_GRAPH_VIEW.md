# Grafo de commits en GUI y TUI

Abre **Git** en la GUI o **Estado Git** en la TUI y pulsa **G**. En la GUI
también puedes elegir **Grafo**. Los carriles muestran la ascendencia de la
referencia seleccionada. **◆** marca una integración y **○** marca un corte de
ascendencia. La fila muestra el SHA abreviado, el resumen y el autor.

Pulsa **R** o elige **Referencias** en la GUI para seleccionar **HEAD**,
una rama local o remota, o una etiqueta. **Intro** abre la referencia
seleccionada y **Esc** cierra la lista. Se admiten etiquetas ligeras y
anotadas que resuelven a un commit. Si una etiqueta apunta a otro tipo
de objeto, el visor indica el fallo y conserva la página anterior.

Usa las flechas y **Intro** para abrir el commit seleccionado. En la GUI
también puedes pulsar la fila. Desde los archivos del commit, **Esc**
regresa a la misma página y selección. En la TUI, si estás en el diff,
**Esc** devuelve primero el foco a la lista de archivos. **N** muestra
la siguiente página. **F5** vuelve a consultar HEAD.

En un merge, pulsa **P** para recorrer sus padres. La GUI también ofrece
el botón **Padre 1/2 ▸**, con la posición y el total del merge abierto.
Cada elección recalcula la lista de archivos frente a ese padre. **Intro**
sobre un archivo compara exactamente esa misma base. El primer padre
es la elección inicial y se admiten hasta ocho padres.

El visor conserva hasta 512 commits de la ascendencia elegida y los
presenta en páginas de 24, con cada hijo antes de sus padres admitidos.
Los carriles continúan al pasar de página. La cabecera distingue el
límite de ascendencia, una copia shallow y las abreviaciones de texto o
carriles.

El núcleo fija la ascendencia con el identificador observado en el
selector. Si la referencia se mueve, la consulta abierta sigue el
identificador elegido. La GUI y la
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

La ampliación de referencias y padres pasó pruebas del núcleo, del
controlador y de ambas interfaces. Se verificaron la apertura de una
rama que se movió después de mostrarse, una etiqueta anotada, el
rechazo de una etiqueta a un blob y el cambio de la lista y el diff
al elegir el segundo padre.

La entrega optimizada está en `target/git-refs-parents-next/release/`.
Pasaron formato, Clippy, 26 comprobaciones de regresión con la GUI
oculta y el recorrido de rama, etiqueta anotada y segundo padre en
la TUI mediante ConPTY privado. Las fuentes y los metadatos de los
repositorios de prueba conservaron sus bytes.

## Crear referencias desde un commit

En el grafo, selecciona un commit y pulsa **B** para crear una rama
local o **T** para crear una etiqueta ligera. La GUI también ofrece
los botones **Crear rama** y **Crear etiqueta**. Escribe el nombre
y pulsa **Intro** para revisar el nombre completo y el SHA de destino.
En la GUI, **Ctrl+Intro** confirma. En la TUI, usa **F2**. **Esc**
descarta el formulario.

El motor crea una referencia nueva bajo el bloqueo Git y comprueba
colisiones con referencias sueltas o empaquetadas. La creación conserva
la rama activa, HEAD, el índice, los archivos de trabajo y los reflogs.
El grafo actualiza el selector al terminar.
