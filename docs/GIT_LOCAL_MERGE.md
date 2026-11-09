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
- **Conflicto:** la revisión lista las rutas y deshabilita la confirmación.
  No se crean marcadores, etapas de conflicto ni `MERGE_HEAD`. La resolución
  interactiva de BASE/ACTUAL/ENTRANTE/RESULTADO queda como siguiente bloque.

La vista previa no escribe objetos ni toca el índice. La confirmación vuelve a
comprobar las referencias, HEAD, el índice y los archivos afectados. Las
ediciones sin guardar en CIDE bloquean la operación. Una edición en disco, un
cambio preparado o un archivo sin seguimiento que coincidan con una ruta que
se debe reemplazar bloquean la escritura; el resto de los cambios locales se
conserva. En una fusión divergente, los cambios preparados ajenos quedan en
el índice y no entran en el commit de fusión.

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
`python scripts/build-cide-local.py --output target/git-local-merge-next/release`.
