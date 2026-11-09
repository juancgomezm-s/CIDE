# Fusión local de ramas en CIDE

En **Git → Grafo → Referencias**, selecciona la rama que quieres incorporar a
la activa. En la GUI usa **Fusionar rama** o **M**; en la TUI usa **M**.
**Intro** abre la revisión. La revisión identifica las dos ramas y sus commits,
el resultado previsto y los archivos que cambiarían. Confirma con
**Ctrl+Intro** en la GUI o **F2** en la TUI. **Esc** cierra la revisión.

El núcleo Rust embebido calcula la ascendencia y distingue:

- **Ya incorporada:** la rama activa contiene el commit entrante. No hay
  confirmación ni escritura.
- **Avance directo:** la referencia activa se adelanta al commit entrante.
- **Ramas divergentes:** se calcula una base común y se crea un commit con dos
  padres, primero el activo y luego el entrante. En archivos regulares, el
  motor puede combinar ediciones de líneas no superpuestas. Conserva los bytes,
  incluido CRLF y la ausencia de salto al final.
- **Conflicto:** abre cada ruta con **Intro**. **Tab** recorre **Base**,
  **Actual**, **Entrante** y **Resultado**. **Ctrl+U** toma la versión
  que estás viendo como resultado. En la GUI puedes usar también los botones.

**Actual** corresponde al commit de la rama activa. **Resultado** empieza con
esa versión. Puedes editar y pegar texto,
usar **Intro** para un salto de línea y **Ctrl+T** para un tabulador.
Acepta el archivo desde **Resultado** con **Ctrl+Intro** en la GUI o **F2**
en la TUI. En la lista, la misma tecla genera la revisión final cuando todos
los archivos están resueltos. La publicación requiere confirmar desde esa
revisión. **Esc** vuelve al borrador de resoluciones desde la revisión final.

La edición admite UTF-8 de hasta 64 KiB por archivo. Para archivos mayores
o con otros bytes puedes elegir una versión completa, incluida una versión
en la que el archivo no existe.

En una colisión **Archivo/carpeta**, cada pestaña indica si la ruta contiene
un archivo, una carpeta o está ausente en ese commit. La vista de carpeta
muestra los archivos que incluye. **Ctrl+U** elige la versión completa y
**Resultado** muestra qué estructura quedará al confirmar.

Para convertir una carpeta en archivo, CIDE verifica los archivos que
retirará y guarda sus originales en `.git`. La operación se detiene si
detecta cambios locales, archivos sin seguimiento o carpetas ajenas dentro
de la ruta. Solo retira carpetas vacías. La recuperación vuelve a comprobar
los respaldos y continúa desde el último estado verificable. Otros conflictos
estructurales, como los renombres incompatibles, quedan pendientes.

La vista previa no escribe objetos ni toca el índice. La confirmación vuelve a
comprobar las referencias, HEAD, el índice y los archivos afectados. Las
ediciones sin guardar en CIDE bloquean la operación. Una edición en disco, un
cambio preparado o un archivo sin seguimiento que coincidan con una ruta que
se debe reemplazar bloquean la escritura; el resto de los cambios locales se
conserva. En una fusión divergente, los cambios preparados ajenos quedan en
el índice y no entran en el commit de fusión.

Las resoluciones se guardan en el borrador mientras está abierto. El resultado
final se registra con su árbol exacto al confirmar. Si una publicación
interrumpida se reanuda, usa ese mismo árbol, incluidas las ediciones del
borrador.

Si falla una publicación después de crear el registro de recuperación, elige
la misma rama entrante para **Reanudar fusión**. El registro y los respaldos se
guardan dentro de `.git`. El motor comprueba los identificadores, las huellas
del índice y los bytes esperados antes de retomar. Si encuentra una edición
posterior, la conserva y detiene la recuperación. Una fusión interrumpida
bloquea las demás escrituras Git de CIDE hasta que se complete. El reflog de la
rama y el de HEAD registran la publicación cuando están habilitados. El estado
Git, el grafo y el explorador se actualizan al terminar.

El flujo requiere un repositorio ordinario de Windows, una rama activa, un
índice presente y rutas admitidas por la transacción de archivos. El recorrido
de ascendencia está acotado a un máximo de 8192 commits y la operación admite
hasta 128 rutas afectadas. Varias bases de fusión, árboles sin base común,
repositorios parciales, enlaces en rutas afectadas, conversiones de texto y
conductores de merge requieren otro flujo. No hay seguimiento general de
renombres; los posibles renombres en competencia se rechazan. El motor no
ejecuta `git.exe`, hooks ni acciones de red.

Validación local en Windows con Rust 1.88.0: `python scripts/check-local.py
--steps fmt clippy test --jobs 2` pasó con 900 pruebas y tres auxiliares
marcados como ignorados en la ejecución directa, invocados por sus pruebas
padre. La primera corrida se detuvo porque el entorno restringido no permitió
un enlace duro en una prueba preexistente; la corrida completa posterior pasó
con acceso al fixture. Se verificaron la revisión y confirmación en ambos
frontends, los conflictos, los cambios locales, la obsolescencia y la
reanudación tras publicar el índice. Las comprobaciones se ejecutaron en
segundo plano, sin escritorio, repositorios personales ni GitHub Actions.

Para generar los ejecutables usando la caché compartida:
`python scripts/build-cide-local.py --output target/git-merge-structural-next/release`.

La resolución interactiva pasó la suite local de Windows con Rust 1.88.0
mediante `python scripts/check-local.py --steps fmt clippy test --jobs 2`.
Pasaron 901 pruebas. Los tres auxiliares se ejecutaron desde sus pruebas padre.
Las pruebas cubrieron la edición y confirmación en ambos frontends, el rechazo
de cambios locales posteriores a la revisión y la recuperación del resultado
editado tras publicar el índice.

La validación de archivo/carpeta pasó 902 pruebas con el mismo comando.
Los recorridos de GUI y TUI cubrieron las dos conversiones. El núcleo
comprobó el rechazo de ediciones locales, la protección de un archivo
creado durante la publicación y la recuperación posterior.
