# Auditoría y simplificación de menús M2

La composición anterior repetía once entradas entre Proyecto y Ver. La GUI
añadía después todos los comandos implementados del registro nativo: pasos de
plantillas, aceptación de resultados, direcciones del rectángulo, foco e índices
de pestaña acababan expuestos junto a las operaciones de entrada.

La lista compartida pasa de 109 entradas (98 identidades únicas) a 73 entradas
sin duplicados. Proyecto reúne la actualización del explorador y la navegación
de código: símbolos, referencias, Llama a, Llamado por e inclusiones. Ver reúne
la presentación, grupos y comparaciones. Editar conserva las operaciones de
texto, comentarios, plantillas y el inicio de selección rectangular.

La GUI toma esa selección, filtra los comandos que admite y separa Navegación
y Git. Añade de forma explícita las tres opciones de búsqueda y anterior/siguiente
pestaña. No vuelve a agregar el registro entero. Las acciones retiradas de los
menús conservan su identidad, handler, atajo cuando existe y entrada de paleta;
no se alteran documento, transacciones ni proveedores.

## Flujos comprobados en fuente

| Grupo | Ruta real |
|---|---|
| Archivo | Workbench y flujos de abrir, guardar, guardar todo, codificación y cierre propios de cada frontend |
| Texto, líneas, comentarios y rectángulo | Comandos del Workbench con propuestas/transacciones existentes |
| Búsqueda de documento y proyecto | Controladores S0/S1 y vistas previas versionadas |
| Símbolos, referencias, llamadas e inclusiones | Servicio léxico opcional y resultados versionados; no equivale a análisis semántico de AVM/LSP |
| Plantillas | Asistente compartido de revisión y aplicación en una transacción |
| Git y comparación de abiertos | Proveedores existentes de solo lectura y controladores cancelables |
| Panels y grupos | Estado de presentación, foco y vistas existentes |

Base de datos/esquema tiene disponibilidad `never` y Terminal anuncia que está
pendiente; ambas se retiran de los menús. Compilar solo anunciaba el requisito M3 y Ayuda estaba vacía. Ambos títulos
se ocultan también en TUI. Sus macros siguen en el registro y la paleta;
invocarlas explica la capacidad pendiente, sin abrir un overlay vacío.
Los cuatro comandos Progress conservan su condición de perfil AVM. No se declara implementación de esas rutas
ni se activa AVM, compilador, terminal o conexión de base de datos con esta limpieza.

## Validación local

Se añade una regresión compartida que exige identidades únicas y conserva
Llamado por, inclusiones y renombrado. La regresión nativa exige que toda entrada
visible pertenezca a un handler implementado, no esté duplicada y omita los pasos
contextuales que inflaban la composición. El integrador ejecuta formato, pruebas
de `jc-app`, `jc-tui` y `jc-gui`, y el recorrido nativo de menús. Este documento
no constituye aceptación física/global de M2. El cierre aprobó 152 pruebas
TUI en ejecución serial, tres casos compartidos seleccionados por menú,
las pruebas GUI de definiciones, Clippy y release. El recorrido nativo de
interacción pasó 16 comprobaciones, incluidas composición y routing de menús.
Registros y limitaciones en [VALIDACION](VALIDACION.md).
