# Registro de decisiones

Las decisiones aceptadas describen requisitos o dirección arquitectónica. Las pendientes requieren información o pruebas; no son capacidades implícitas de M0.

## D01 — Núcleo de edición independiente

**Aceptada.** Rust se usa para el núcleo inicial. Los documentos y operaciones no dependen de widgets ni de una sesión de Progress. La interfaz concreta sigue pendiente.

## D02 — Bytes como representación canónica inicial

**Aceptada para el prototipo.** Los fuentes del usuario no son UTF-8 y todavía no conocemos su página exacta. La representación debe conservar bytes arbitrarios. Una piece table permite mantener fragmentos originales, pero necesita una capa de interpretación para operaciones textuales y visuales.

**Consecuencia:** no adoptar Ropey como almacenamiento directo de esos archivos sin una decisión explícita sobre conversión y conservación. Tampoco equiparar offset de byte con columna visual.

## D03 — Reutilización selectiva de Lime

**Aceptada.** Portar comportamientos pequeños con pruebas y procedencia identificada. No portar automáticamente el buffer de runas, los canales por consulta, el editor global ni la integración Python/Sublime. Cada componente derivado mantiene los avisos correspondientes.

**Pendiente:** decisión del propietario sobre la licencia del código nuevo. Esta documentación no asigna una licencia general al proyecto.

## D04 — Progress como proveedor de compilación y ejecución

**Aceptada.** Un adaptador y auxiliares ABL coordinan la instalación seleccionada por proyecto. La validación de sintaxis real pertenece al compilador de esa instalación. JC-IDE no implementa el protocolo propietario de la BD ni un compilador ABL propio.

**Consecuencia:** preservar el contexto de conexión, versión, includes, arranque y codificación; verificar compatibilidad por versión. Los resultados de documentación o pruebas Rust no certifican los auxiliares ABL.

## D05 — Catálogo de esquema local

**Aceptada.** La escritura y las sugerencias consultan metadatos locales actualizables. La extracción se realiza bajo demanda. Un `.df` es una fuente posible del catálogo; no sustituye por sí solo una BD de esquema para el compilador.

## D06 — Servicios opcionales y consumo observable

**Aceptada.** El editor abre archivos sin requerir Progress, Git, LSP ni agentes activos. Se medirá el consumo de cada proceso y el total. La posibilidad de mantener una AVM abierta es una optimización posterior con coste explícito.

## D07 — Interfaz Windows

**Pendiente.** Evaluar una interfaz nativa con motor Rust frente a un componente existente como Scintilla. Si se adopta Scintilla, documentar que el motor de edición es C++ y redefinir el alcance del núcleo Rust. Los presupuestos de memoria no prueban por adelantado cuál opción será mejor.

## D08 — Integración con OmniCore

**Diferida.** OmniCore conserva su núcleo C# y sus contratos de sesión/agentes. La integración futura intercambiará solicitudes, revisiones y resultados. Las modificaciones propuestas deberán respetar la codificación y mostrarse como diffs. No se unifica automáticamente el undo de documentos con la reversión de tareas de agentes.

## D09 — Multilenguaje desde la base

**Aceptada por requisito explícito.** ABL tiene prioridad de integración y no exclusividad. El registro inicial contempla ABL, C#, Rust, JavaScript/TypeScript, Python, HTML/CSS, SQL, XML, JSON, Markdown y texto plano. Cada proveedor mantiene sus capacidades y dependencias; el núcleo de bytes no incorpora reglas de un lenguaje concreto.

**Consecuencia:** distinguir identificación/perfiles de resaltado, parsing, LSP y depuración. Los servicios externos se inician cuando se necesitan y la codificación se conserva por archivo.
