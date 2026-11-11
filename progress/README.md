# Integración Progress ABL

Este directorio contiene **un prototipo original**, inspirado en la separación
editor/AVM que utiliza PIEW. No incorpora binarios de Progress ni el código de la
interfaz de PIEW.

- `abl/jc-bridge.p`: recibe una solicitud, comprueba, compila o ejecuta un
  fuente por lotes. También admite solicitudes de esquema.
- `abl/jc-schema.p`: consulta tablas, campos e índices mediante `DICTDB` y
  `NO-LOCK`; produce registros de metadatos, no filas del negocio.
- `examples/hello.p`: fuente ASCII mínimo para la primera prueba manual.
- `examples/manual-smoke.ps1`: prepara una solicitud de comprobación para una
  AVM explícita.

**Estado:** el flujo de comprobación, compilación y ejecución por lotes se
probó con OpenEdge 11.7 x86 en Windows. La consulta de esquema y la
compatibilidad con OpenEdge 10.0B quedan pendientes de validación.

Contrato, limitaciones y comprobaciones pendientes:
[PROGRESS_BRIDGE.md](../docs/PROGRESS_BRIDGE.md).

La biblioteca Rust `jc-progress` lanza el AVM sin shell solo cuando el
usuario ejecuta un comando ABL de un proyecto configurado. El adaptador
permanece fuera del núcleo de texto.
