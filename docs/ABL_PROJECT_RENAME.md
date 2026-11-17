# Renombrar símbolos ABL compartidos

En la GUI y la TUI, abre la carpeta del proyecto y el include guardado.
Coloca el cursor en la declaración del procedimiento, función o variable
global. Pulsa **F2**, introduce el nombre y pulsa **Intro**. La vista previa
muestra el archivo, el renglón y el reemplazo de cada aparición.

**F9** o **Ctrl+Intro** vuelve a resolver el PROPATH y compara el resultado
con la vista previa. Si cambian los archivos, las referencias o el destino,
vuelve a abrir el renombrado y revisa el alcance actualizado. Guarda primero
las ediciones ABL pendientes del proyecto para que la revisión pueda usar
las mismas fuentes que se van a modificar.

Al confirmar, todos los cambios se aplican juntos en el editor. Los archivos
afectados que estaban cerrados se abren como pestañas con cambios. **Guardar
todo** publica los archivos con el flujo de guardado habitual. **Deshacer**
o **Rehacer** desde una pestaña afectada mueve el grupo completo. Si otra
pestaña del grupo tiene una edición posterior, deshaz esa edición primero.
Mantén abiertas las pestañas afectadas para conservar el deshacer del grupo.
La recuperación de sesión guarda las ediciones y el historial agrupado en
una misma generación. El cierre normal confirma la generación final y la
recuperación periódica usa su intervalo habitual.

El recorrido usa el orden de `.cide/propath` y cubre una inclusión directa,
estática y sin parámetros por programa consumidor dentro de la raíz. El editor detiene la operación
cuando encuentra preprocesador dinámico, llamadas indirectas, resolución
ambigua o fuentes fuera del alcance admitido. La revisión admite hasta 256
fuentes en 64 directorios, 256 KiB por fuente, 8 MiB acumulados, 64 reemplazos
y 16 archivos modificados. Los proyectos ABL que excedan estos límites o
usen inclusiones encadenadas requieren ampliar el análisis antes de aplicar
el renombrado.

## Validación local

En Windows con Rust 1.88.0, las pruebas dirigidas cubren la selección por
PROPATH, la vista previa y confirmación en GUI y TUI, el rechazo de un cambio
externo, y la recuperación del deshacer y rehacer de proyecto. OpenEdge 11.7
compiló dos programas consumidores después de guardar el renombrado de una
función de un include. Las comprobaciones se ejecutaron en segundo plano
con fuentes sintéticas. OpenEdge 10.0B queda para la validación final.
