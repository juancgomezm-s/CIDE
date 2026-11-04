# Navegación de código por proveedor

CIDE selecciona el proveedor según el archivo activo, incluso en proyectos
mixtos. GUI y TUI comparten los comandos **Llama a** y **Dependencias e
inclusiones**. Se pueden buscar por nombre en la paleta. En los resultados,
**Intro** sigue el destino y **Ctrl+Intro** abre un archivo al lado.

- **ABL** aporta llamadas `RUN` e inclusiones en listas separadas, además del
  Esquema y la lista combinada de referencias. Una llamada local con declaración
  única permite saltar dentro del documento. Los destinos dinámicos quedan sin
  resolver. La búsqueda externa admite rutas estáticas en el proyecto.
- **JavaScript** aporta al Esquema funciones y clases con nombre de nivel
  superior, y a Dependencias importaciones y reexportaciones ES. Abre rutas
  relativas explícitas desde la carpeta del documento. Los paquetes y las
  importaciones dinámicas con argumento literal se muestran sin resolver. El
  análisis es léxico y puede detenerse ante JSX, plantillas interpoladas o una
  barra ambigua. Quedan pendientes CommonJS y la resolución por manifiestos.

## PROPATH del proyecto ABL

Para declarar el orden sin duplicar un perfil completo, coloca un archivo
`.cide/propath` en la raíz abierta. Es UTF-8 y contiene un directorio relativo
por línea. Se ignoran líneas vacías y comentarios que comienzan con `#`.

```text
# Orden de búsqueda
aplicacion
comun
.
```

Se elige el primer archivo existente dentro de la raíz. Sin ese archivo, las
vistas nuevas prueban la carpeta del fuente y después la raíz. La vista
combinada anterior sigue ofreciendo la selección entre candidatos en ese caso.
La configuración se lee al activar la referencia. Admite hasta 32 entradas y
8 KiB. La apertura vuelve a comprobar la ruta del archivo mediante el lector
confinado del proyecto.

## Siguientes bloques

**Llamado por** requiere un recorrido cancelable del proyecto que use las mismas
reglas de resolución y entregue relaciones con archivo, revisión y rango. Ese
recorrido también debe distinguir posibles puntos de entrada de ciclos y de
destinos inciertos. C# y PHP podrán registrar sus capacidades sobre el mismo
contrato. El catálogo de esquemas y sus proveedores OpenEdge, PostgreSQL y
MySQL se desarrollan como un componente separado.
