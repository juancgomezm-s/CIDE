# Vínculos a tipos, inclusiones y archivos del proyecto

Los mismos vínculos sirven en GUI y TUI. Doble clic, Ctrl+clic y
Ctrl+Intro solicitan la navegación mediante el controlador compartido;
Ctrl+izquierda vuelve al origen y Ctrl+derecha recorre el camino de regreso.
El código conserva sus bytes y sus cambios sin guardar.

## Destinos

- ABL: clases en AS / AS CLASS, NEW, USING, INHERITS, IMPLEMENTS,
  RETURNS y tipos de retorno de METHOD. Se reutilizan USING y PROPATH;
  la declaración debe corresponder al archivo .cls observado.
- ABL: inclusiones .i con ruta literal, también con parámetros de inclusión.
  Las macros en la ruta no se interpretan como archivos estáticos.
- JavaScript / TypeScript: importaciones y require de rutas relativas.
  TypeScript conserva la resolución .js hacia su fuente .ts existente.
- PHP: include, include_once, require y require_once con ruta literal,
  incluida la concatenación literal con __DIR__.
- Rust: include! de un archivo .rs y mod externo en un archivo nombre.rs.
  Un módulo inline no se convierte en un vínculo de archivo.
- Java: importación de una clase y de miembros static, mediante las raíces
  de código configuradas del proyecto.
- C#: alias de tipo y using static cuando se resuelven a un fuente del
  proyecto. Un using ordinario de espacio de nombres no identifica un archivo.

Se excluyen los prefijos de bibliotecas del sistema reconocidos (System,
Microsoft, java, javax, jdk, sun, std y core), las importaciones ABL FROM
ASSEMBLY, los paquetes npm sin ruta relativa y las rutas de red o calculadas.
No se abre ni descompila una biblioteca binaria. La apertura sigue confinada
al proyecto; no se buscan destinos en los directorios de Windows o del SDK.
La navegación semántica restante conserva el recorrido LSP existente.

## Implementación y límites

La clasificación de operandos de archivo se comparte entre el pintado y
el trabajador. El pintado no lee el disco. Se mantienen el límite de 32
vínculos por renglón, el análisis de renglones de hasta 4 KiB, las capturas
de servicios de hasta 2 MiB y la exclusión existente para fuentes paginadas.
Los literales de ruta se admiten como UTF-8/ASCII; no se interpreta código
para calcular rutas ni se certifica la semántica de un import ambiguo.

Las aperturas con versión fijada conservan la huella del contenido. Abrir
un archivo y ubicar su declaración es un único salto del historial:
no se intercala una visita artificial al byte cero del destino.

## Validación local

El bloque añade matrices de GUI y TUI con once recorridos: propiedad ABL,
retorno de método, NEW, inclusión, importaciones de Java y C#, módulos JS/TS,
require PHP e include!/mod de Rust. Verifican destino, regreso al byte
original y conservación de los fuentes. Las comprobaciones de léxico
excluyen comentarios, tipos incorporados y bibliotecas del sistema.
Los resultados finales y la comprobación de escritorio se registran al
cerrar la entrega; una prueba de entrada interna no equivale a un clic físico.

Cierre local: pasaron 14 pruebas del controlador, 9 GUI (8 casos de entorno
omitidos), 16 TUI (9 omitidos), 6 del caché léxico y la resolución de clases
por catálogo. La prueba nueva de rutas estáticas pasó con las exclusiones
de comentarios y paquetes del sistema. Clippy all-targets pasó en los cinco
crates afectados; la compilación debug también pasó.

Computer Use verificó sobre la GUI real el doble clic y Ctrl+Intro de una
inclusión y de la clase de una propiedad; el doble clic de un retorno METHOD
y de NEW, Ctrl+izquierda con regreso exacto en un paso y Ctrl+derecha.
La instancia salió normalmente y los fuentes permanecieron intactos. No se
presenta Ctrl+clic físico como aprobado: la restricción de integridad del
Teclado en pantalla sigue pendiente. Los tres accesos comparten el mismo
controlador, pero esa coincidencia no sustituye la prueba física faltante.
