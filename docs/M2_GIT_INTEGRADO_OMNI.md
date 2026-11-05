# Integración Git basada en OmniCore y OmniCoder

Juan pidió integrar el flujo dentro de CIDE y reutilizar el trabajo de sus
otros proyectos, eliminando la selección manual de `git.exe`.

## Fuentes locales revisadas

- `C:/Users/juanc/source/repos/OmniCoder-kp-remeasure-v2/src/OmniCoder.Core/Git/GitCommandRunner.cs`
  usa Git instalado, argumentos separados, proceso sin ventana ni shell,
  ejecución fuera de UI, límites y cancelación; su explicación descarta libgit2.
- `C:/Users/juanc/source/repos/OmniCoder-kp-remeasure-v2/src/OmniCoder.Core/Git/GitClient.cs`
  y los modelos de presentación integran operaciones y resultados en la interfaz.
- `C:/Users/juanc/source/repos/OmniCore/src/OmniCore.Infrastructure/GitWorktreeStore.cs`
  usa `ProcessLaunch("git", ...)`, argumentos separados, entrada cerrada,
  cancelación y espera fuera de UI.

La copia local de OmniCoder revisada declara licencia MIT, copyright 2026
Juan Gómez. No se pegaron clases C# ni se añade su runtime. Se adapta el flujo
de integración a Rust y se conserva el transporte contenido ya implementado
por CIDE. Es reutilización del comportamiento, no un port literal del runner.

Ambos proyectos usan Git instalado; ninguno de esos archivos implementa un
motor Git embebido. Este bloque tampoco afirma distribuir un Git binario propio.

## Implementación

`jc-app::git_runtime` resuelve una ruta absoluta antes de entregar al runner:
ruta de paquete `git/cmd/git.exe`, instalaciones convencionales de Git para
Windows y directorios absolutos del PATH del usuario. La búsqueda está limitada
a 70 candidatos y 64 entradas de PATH. Omite entradas relativas y candidatos
dentro del proyecto o del directorio de trabajo; no ejecuta un Git del proyecto.
No descarga, instala, lanza shell ni altera variables del sistema.

GUI descubre al iniciar cuando no existe una configuración guardada/CLI;
TUI lo hace después de recuperar la sesión. Se conserva la opción avanzada
`--git-executable`. En GUI, la antigua acción de configurar ahora detecta
y ejecuta la consulta, sin diálogo de archivos. Si Git no está disponible,
se informa desde el editor. El menú dice «Detectar Git instalado».

Se mantienen la actualización por eventos, resultados versionados, límites,
trabajador acotado y lectura de estado/diffs. Las operaciones con escritura
de OmniCoder/OmniCore no se incorporan al bloque M2; siguen en M5.

## Validación

Prueba del resolver con candidatos propios: exclusión de proyecto/directorio
actual, rutas relativas y archivos ausentes. No cambia el entorno global.
GUI: 75 pruebas de biblioteca y seis de entrada aprobadas. Clippy de todos
los destinos app/GUI/TUI, formato y release de ambos frontends aprobaron.

El recorrido GUI integral con Git instalado pasó 135 comprobaciones, incluida
`git_installed_runtime_detected_without_executable_picker`, estado automático,
contador persistente, diff HEAD/índice y fuentes/metadatos intactos. Informe:
`work/native-gui-smoke/20261105T212037Z-16c19901759f`.
Logs: `work/git-integrated-*.log`. No se declara una nueva aceptación física
global ni un recorrido ConPTY de detección automática TUI.

Ejecutables: `target/git-integrated-next/release/cide-gui.exe` y `cide-tui.exe`.
Se conservan los anteriores. La guía de seleccionar ejecutable queda sustituida
por este flujo automático.
