# Investigación

## Lime Text

Lime Text quedó abandonado en la práctica. Su código, sin embargo, contiene piezas útiles: regiones/selecciones, comandos, undo/redo, keybindings y separación entre backend y frontends.

JC-IDE no porta Lime completo. Se reutiliza selectivamente comportamiento pequeño y atribuible. El buffer nuevo es byte-oriented porque los fuentes objetivo no se pueden asumir UTF-8.

Hallazgos que no deben copiarse sin rediseño:
- mezcla de offsets en bytes y posiciones textuales;
- tratamiento incompleto de codificaciones legacy;
- integración parcial de plugins Sublime/Python;
- frontends incompletos y flujos de cierre/guardado pendientes.

## PIEW

PIEW sirve como referencia conceptual: el editor era sencillo y delegaba a Progress la comprobación, compilación y ejecución. El explorador de esquema podía alimentarse de metadatos exportados.

La dirección de JC-IDE es conservar esa sencillez: editor ligero + auxiliares ABL + catálogo local.

## Progress

La instalación de OpenEdge del proyecto es la autoridad para compilar ABL y resolver referencias estáticas a tablas, campos e includes. El editor puede consultar metadatos de esquema mediante ABL y mantenerlos localmente para navegación y autocompletado.

OpenEdge 10.0B y 11.7 se consideran objetivos separados y deben probarse en instalaciones reales.
