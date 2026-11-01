# Resumen de JC-IDE

JC-IDE es un proyecto de editor de código multilenguaje para Windows, con atención prioritaria a Progress OpenEdge/ABL. Su prioridad es abrir, editar y guardar fuentes con un consumo reducido, conservando sus bytes y la codificación que utiliza cada archivo. La integración con Progress debe permitir comprobar, compilar, ejecutar y consultar el esquema de las bases de datos desde una interfaz sencilla.

## Punto de partida

Esta entrega es la base técnica y documental del proyecto, denominada **M0**. No constituye todavía un IDE gráfico utilizable ni un compilador de ABL. El alcance del código inicial se concentra en una biblioteca de edición de bytes y en el contrato de un adaptador de Progress. El estado verificable y las pruebas efectuadas se registran en [VALIDACION.md](VALIDACION.md).

La investigación previa examinó Lime Text como donante de lógica de edición y PIEW como referencia de integración con Progress. La decisión es reimplementar selectivamente comportamientos útiles, conservar las atribuciones de los componentes portados y construir una arquitectura pequeña que responda a los requisitos propios. No se plantea portar todo Lime ni reproducir su compatibilidad parcial con plugins de Sublime.

## Requisitos acordados

- Conservar fuentes que **no son UTF-8**. La página de códigos concreta de los proyectos del usuario sigue pendiente de identificación; no se asume Windows-1252, ISO-8859-1 ni IBM850.
- Separar el documento en disco, su interpretación como texto y las coordenadas usadas para dibujar y editar.
- Usar un núcleo Rust independiente de la interfaz gráfica.
- Soportar varios lenguajes desde el diseño: ABL, C#, Rust, JavaScript/TypeScript, Python, HTML/CSS, SQL, XML, JSON, Markdown y texto plano. Detectar un lenguaje o tener su perfil no equivale a disponer de resaltado, análisis, compilación o depuración para él.
- Aprovechar la instalación de Progress del proyecto mediante auxiliares ABL para la compilación, el esquema y la ejecución.
- Consultar un catálogo local de tablas, campos e índices durante la escritura; actualizarlo por una acción explícita.
- Mantener los servicios costosos bajo demanda y medir tanto el editor como sus procesos auxiliares.
- Considerar OpenEdge 10.0B y 11.7 como entornos objetivo. La compatibilidad debe probarse con cada instalación real; no está certificada por disponer de documentación de otra versión.

## Experiencia inicial buscada

El usuario configura un perfil con el ejecutable de Progress, `.pf`, `.ini`, `PROPATH`, codificaciones y procedimiento de entrada. Puede editar sin iniciar una AVM. Un catálogo obtenido de la BD o de un `.df` permite explorar nombres y completar campos. Al pedir comprobar o ejecutar, JC-IDE invoca el entorno seleccionado y presenta sus resultados.

Una base con solo esquema puede servir para compilar referencias estáticas sin cargar datos de negocio. El catálogo del editor y esa base tienen responsabilidades distintas: leer un `.df` no convierte al editor en el compilador de Progress.

## Evolución posterior

El soporte multilenguaje es un requisito base; sus capacidades se completan por etapas mediante proveedores independientes. Después del registro y la detección vienen el resaltado, la navegación, las tareas y la integración opcional de servicios de lenguaje. También se contemplan búsquedas de proyecto, Git y una integración opcional con OmniCore. OmniCore mantiene su núcleo C# y su propia gestión de sesiones y agentes; JC-IDE no implica reescribirlo. El undo de edición y la reversión de una operación de agente también requieren contratos separados.

La elección de interfaz gráfica permanece abierta. Las opciones evaluadas y sus consecuencias están en [ARQUITECTURA.md](ARQUITECTURA.md); los hitos y criterios de aceptación están en [PLAN.md](PLAN.md).
