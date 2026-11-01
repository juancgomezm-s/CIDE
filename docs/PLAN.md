# Plan de desarrollo

Los hitos de este documento pertenecen exclusivamente a JC-IDE. Son una secuencia propuesta, sin estimaciones de fechas ni porcentajes de avance. Un hito se cierra con evidencia de sus criterios de aceptación.

## M0 — Base verificable

**Alcance:** repositorio, documentación, biblioteca de bytes, comportamiento de regiones e historial, registro multilenguaje, adaptador inicial y pruebas automatizadas posibles sin instalar Progress.

**Criterios de aceptación:**

- Compilación, formato y pruebas Rust reproducibles con el toolchain registrado.
- Abrir bytes arbitrarios y reconstruirlos sin cambios; no exigir UTF-8.
- Insertar, eliminar, reemplazar, deshacer y rehacer sin pérdida de bytes.
- Rechazar operaciones fuera de rango sin modificar documento ni historial.
- Comparar secuencias de ediciones con un modelo independiente sencillo.
- Documentar qué auxiliares ABL son experimentales y no se han ejecutado.
- Identificar los elementos portados y conservar sus atribuciones.
- Detectar los perfiles iniciales de ABL, C#, Rust, JavaScript/TypeScript, Python, HTML/CSS, SQL, XML, JSON, Markdown y texto plano, con pruebas para extensiones y nombres especiales implementados. Declarar por separado las capacidades que aún faltan.

**Salida:** una base para continuar el desarrollo, todavía sin editor gráfico.

## M1 — Archivo y texto interpretado

**Alcance:** identificar las codificaciones reales, gestión de archivos, posiciones textuales y guardado.

**Criterios de aceptación:**

- Corpus pequeño de fuentes reales autorizado, con codificación confirmada y hashes de referencia; los archivos privados no se incluyen en el repositorio.
- Abrir/guardar sin editar conserva el hash; editar una zona conserva los segmentos no afectados.
- Preservación de CRLF/LF, ausencia de salto final y bytes no interpretables.
- Inserción de caracteres no representables produce error explícito, sin pérdida silenciosa.
- Pruebas de fallo de escritura, sustitución del archivo y modificación externa.
- Definición de límites del historial y medición con muchas ediciones.

## M2 — Editor Windows mínimo

**Alcance:** decidir la interfaz con evidencia y entregar una ventana de edición funcional.

**Criterios de aceptación:**

- Abrir, editar, guardar, cerrar con cambios, deshacer y rehacer desde la interfaz.
- Cursor, selección, portapapeles, desplazamiento y líneas largas correctos.
- Manejo de acentos, tabulaciones e introducción de texto comprobado en Windows.
- Renderizado de la región visible sin reconstruir todo el documento por cada tecla.
- Selección manual de lenguaje y resaltado mediante proveedores, con alcance declarado por lenguaje y sin cargar todos los analizadores al arrancar.
- Medición de arranque y memoria con archivos y equipo descritos.

La elección de Scintilla requiere actualizar la decisión arquitectónica sobre qué parte del núcleo pertenece a Rust. La elección de renderizado propio requiere asignar trabajo a IME y accesibilidad.

## M3 — Entorno Progress funcional

**Alcance:** perfiles, comprobación, compilación y ejecución con instalación real.

**Criterios de aceptación:**

- Probar por separado los perfiles de OpenEdge 10.0B y 11.7 que se acuerden soportar.
- Validar programa correcto y errores de sintaxis, tabla/campo e include; comprobar archivo y posición mostrados.
- Mantener codificación, `PROPATH`, aliases y contexto del proyecto.
- Comprobar que ejecutar usa el código recién compilado incluso si existe un `.r` anterior.
- Separar salida temporal de comprobación y salida de compilación solicitada.
- Probar un programa independiente y otro que requiere inicialización de la aplicación.
- Manejar cancelación, errores de conexión, respuestas incompletas y solicitudes obsoletas.

Las pruebas requieren un entorno de desarrollo apropiado. Los scripts incluidos no equivalen a haber ejecutado esa validación.

## M4 — Esquema y asistencia ABL

**Alcance:** catálogo local de BD, tablas, campos e índices; autocompletado inicial y navegación.

**Criterios de aceptación:**

- Exportar metadatos mediante consultas sin modificar la BD.
- Mostrar tipos, formatos y composición de índices; distinguir bases mediante nombres lógicos.
- Actualizar el catálogo de forma explícita y mostrar su procedencia/antigüedad.
- Completar campos sin conexión durante la escritura.
- Leer `.df` solo con el subconjunto/formato que realmente se implemente y documente.
- Diferenciar campos de BD, buffers, variables y temp-tables al ampliar el análisis contextual.

Después pueden usarse XREF/LISTING para referencias e información de acceso. El protocolo LSP es una posible interfaz de exposición futura, no un requisito previo del editor.

## M5 — Servicios opcionales

**Alcance:** Git, búsquedas de proyecto, servicios avanzados de los distintos lenguajes, ejecución de tareas e integración con OmniCore. El soporte multilenguaje comienza en M0; este hito amplía sus capacidades.

**Criterios de aceptación:** cada servicio permanece desacoplado, tiene inicio/cierre definidos y no altera el consumo basal cuando no se usa. Las ediciones de agentes se muestran como cambios revisables, conservan la codificación y se aplican contra una revisión conocida. El editor no sustituye el historial de operaciones del agente por su propio undo.

Los proveedores deben declarar si su compilador, formatter o servidor de lenguaje está disponible. La instalación de uno no debe ser requisito para editar otro lenguaje. Las pruebas deben distinguir detección, resaltado y capacidades semánticas; mostrar un nombre de lenguaje en la barra de estado no acredita completado o depuración.

## Presupuestos propuestos

Estos valores son **objetivos exploratorios, no resultados medidos ni promesas**:

| Escenario | Objetivo inicial | Cómo medir |
|---|---|---|
| Editor abierto con un archivo pequeño | Menos de 50 MiB de memoria privada | Registrar archivo, versión, sistema, equipo y procesos incluidos |
| Arranque del editor | Menos de 0,5 s hasta poder editar | Distinguir arranque frío/caliente y medir varias repeticiones |
| Editor con Progress activo | Sin umbral fijado todavía | Informar editor, AVM y total del conjunto |
| Edición intensiva y archivos grandes | Sin umbral fijado todavía | Latencia, fragmentos, memoria de historial y crecimiento de cachés |

Antes de optimizar hay que establecer una línea base. No comparar el editor aislado con otro producto que tenga análisis, terminales o conexiones activas sin describir las diferencias del escenario.
