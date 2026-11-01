# Integración Progress ABL

Este directorio contiene **un prototipo original**, inspirado en la separación
editor/AVM que utiliza PIEW. No incorpora binarios de Progress ni el código de la
interfaz de PIEW.

- `abl/jc-bridge.p`: recibe una solicitud, comprueba/compila un fuente o coordina
  la lectura del esquema de las bases conectadas.
- `abl/jc-schema.p`: consulta tablas, campos e índices mediante `DICTDB` y
  `NO-LOCK`; produce registros de metadatos, no filas del negocio.
- `examples/hello.p`: fuente ASCII mínimo para la primera prueba manual.
- `examples/manual-smoke.ps1`: prepara un trabajo nuevo y ejecuta **solamente una
  comprobación** si se le proporcionan explícitamente la AVM y sus code pages.

**Estado:** los helpers ABL no han sido compilados ni ejecutados aquí porque no
hay una instalación de Progress disponible. Las pruebas Rust validan el
protocolo y la construcción de argumentos, no la compatibilidad de los helpers
con una AVM real. OpenEdge 10.0B y 11.7 son objetivos de verificación separados.

Contrato, limitaciones y comprobaciones pendientes:
[PROGRESS_BRIDGE.md](../docs/PROGRESS_BRIDGE.md).

La biblioteca Rust `jc-progress` construye comandos sin un shell y no lanza una
AVM automáticamente. Este adaptador permanece fuera del núcleo de texto; otros
lenguajes pueden incorporar sus propios adaptadores.
