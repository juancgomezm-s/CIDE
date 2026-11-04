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

## Llamado por

Con un archivo guardado de un proyecto abierto, usa **Llamado por** desde la
paleta o el menú. **Intro** salta a la referencia y **Ctrl+Intro** la abre en
el otro grupo. La lista muestra el archivo que contiene el uso y su línea.

El recorrido cancelable inspecciona los fuentes en disco. Para ABL incluye
`RUN` e inclusiones estáticas con el orden de PROPATH. Para JavaScript incluye
importaciones y reexportaciones relativas admitidas por su proveedor. La
selección revalida la resolución y la huella del fuente antes de abrirlo. Si el
fuente abierto tiene contenido o codificación distintos de la versión analizada,
se rechaza el salto. El alcance es de referencias directas al archivo.

El recorrido se limita a 512 archivos, 128 directorios, 2 MiB por archivo y
16 MiB en total, además de los límites de tiempo y resultados del servicio.
Excluye directorios de Git, `node_modules`, `target` y `.cide`. Los resultados
reflejan el subconjunto léxico de cada proveedor. Las llamadas dinámicas y la
navegación semántica entre símbolos quedan fuera de este recorrido.

## Siguientes bloques

La navegación transitiva debe distinguir posibles puntos de entrada de ciclos
y de destinos inciertos. C# y PHP podrán registrar sus capacidades sobre el mismo
contrato. El catálogo de esquemas y sus proveedores OpenEdge, PostgreSQL y
MySQL se desarrollan como un componente separado.
