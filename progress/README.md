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

**Estado:** el flujo de comprobación, compilación, ejecución por lotes y
consulta de esquema se probó con OpenEdge 11.7 x86 en Windows. La prueba de
esquema usó dos copias locales en solo lectura con aliases. La compatibilidad
con OpenEdge 10.0B queda pendiente de validación.

Contrato, limitaciones y comprobaciones pendientes:
[PROGRESS_BRIDGE.md](../docs/PROGRESS_BRIDGE.md).

La biblioteca Rust `jc-progress` lanza el AVM sin shell solo cuando el
usuario ejecuta un comando ABL de un proyecto configurado. El adaptador
permanece fuera del núcleo de texto.
