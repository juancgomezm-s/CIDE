# Fuentes, temas e inclusiones en declaraciones

Las inclusiones de Esquema se muestran directamente bajo Inclusiones, una
por renglón, con la ruta de la declaración entre llaves. No se crean nodos
intermedios por directorio. Se conserva el orden del código y el destino original;
no cambia la resolución por PROPATH ni la apertura normal o al lado.
Las rutas conservan su escritura y no se recortan según la fuente del editor.
El ancho visible del panel sigue siendo el límite físico de dibujo.

Ver > Preferencias de apariencia incluye Fuente. Abre el selector nativo de
Windows con las familias monoespaciadas instaladas. El tamaño y el zoom siguen
controlándose desde CIDE; los campos de estilo, tamaño y escritura del selector
nativo están deshabilitados. No se instalan ni distribuyen fuentes.
La selección se ve inmediatamente en el editor; Aplicar la conserva y Cancelar
o Esc recuperan la configuración anterior. La sesión v11 conserva la familia;
las versiones anteriores se leen con Consolas como valor predeterminado.

Se agregan Claro nítido, Papel cálido y Grafito profundo. Claro suave también
recibe tonos de sintaxis revisados. La vista previa afecta a toda la ventana,
incluidos menús, exploradores, pestañas, estado y paneles. Las categorías de
sintaxis se conservan, con inclusiones/preprocesador en magenta. El tema CIDE
comparte el magenta con la TUI mediante jc-app::syntax_theme.

## Validación local

Windows x64, Rust 1.88.0 fijado por el repositorio, compilación offline y caché
existente target/embedded-git-next. Registros en work/appearance-includes.

Las pruebas dirigidas comprueban declaraciones planas, orden, destinos originales,
independencia de la fuente del código, controles sin solaparse a DPI 96/144/192,
nombre de fuente acotado y estricto, formatos de sesión anteriores y reversión
de la vista previa. Las paletas nuevas y claras mantienen contraste mínimo
4.5:1 en todas las categorías comprobadas sobre fondo normal y seleccionado.

La primera suite de GUI tuvo dos aserciones antiguas sobre subcarpetas y dos
pruebas WinHTTP bloqueadas en el entorno restringido. Las aserciones se
actualizaron al requisito aprobado; la suite dirigida de Esquema pasó. Los tres
casos WinHTTP dirigidos pasaron en el contexto normal de Windows, sin Internet.
Esto no constituye una nueva ejecución completa de todo el workspace.

Comprobaciones finales: `cargo test -p jc-gui --lib --locked --offline outline_`
(19 aprobadas, una prueba real OpenEdge no ejecutada), filtros `appearance`
(3), `editor_fonts` (1) y recuperación (11). `cargo clippy -p jc-gui -p jc-tui
--all-targets --locked --offline -- -D warnings` y compilación GUI debug aprobados.
`python scripts/smoke-native-gui.py --exe <entrega>/cide-gui.exe --section
selector-input` aprobó los 16 casos dirigidos con ventana oculta propia;
esta comprobación es distinta de la revisión del escritorio.

En la GUI visible, mediante Computer Use, se desplegaron las tres declaraciones
completas de principal.p y se abrieron conexion.i y parametros.i con doble clic.
calculos.i se abrió al lado mediante Ctrl+Intro. Se eligió Courier New en el
selector de Windows, se observó el cambio real en el código y se confirmó junto
con Claro nítido mediante Aplicar. Se revisaron también Papel cálido y Grafito
profundo con vista previa de toda la ventana y Cancelar recuperó el tema previo.
Se observaron las inclusiones magenta en el editor y en el minimapa.
Los cinco archivos del proyecto sintético conservaron sus hashes en disco.
La instancia visible tiene una edición ajena sin guardar; se conserva abierta.

La última revisión del selector verificó por la GUI y accesibilidad que Estilo,
Tamaño y Alfabeto están deshabilitados, con la muestra visible. Se canceló el
selector y las preferencias, y esa segunda instancia cerró normalmente con
código 0. Entrega local final: `target/appearance-fonts-final/debug/cide-gui.exe`,
con su manifiesto y los 391 componentes verificados del runtime OmniCore.

No se generó MSI ni se publicó en GitHub para este bloque. Los paquetes 0.1.13
anteriores conservan su contenido; estos cambios requieren una nueva entrega.
