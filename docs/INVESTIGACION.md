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

## ecode / eepp

Revisión de código del 2026-11-01: [ecode](https://github.com/SpartanJ/ecode/tree/ae0a5cda57c11a3f074eb383708a3dfb017ec4df) remite al código en [eepp](https://github.com/SpartanJ/eepp/tree/d35cbfed51a7aefa994953bbfa0d1bf373445cb5). El editor está repartido entre `src/tools/ecode`, `src/eepp/ui/uicodeeditor.cpp`, `src/eepp/ui/doc` y `src/eepp/ui/tools/uicodeeditorsplitter.cpp`.

Aprovechar como referencia: documento compartido entre vistas, layout y cachés, render por rango visible, palette conectada a comandos y trabajos de búsqueda/proveedores fuera de la UI. No trasladar automáticamente su representación normalizada de texto, agrupación temporal de undo, toolkit ni plugins. Nuestro contrato de bytes y la propiedad independiente de selección por vista son requisitos propios.

La [auditoría detallada](AUDITORIA_ECODE_ARQUITECTURA.md) fija enlaces a fuentes, límites de la inspección y diferencias respecto a JC-IDE. No se compiló ni midió ecode; no se atribuye una ventaja de velocidad sin comparación.

## Sublime Text: referencia visual

Se conserva la dirección visual de [Sublime Text](https://www.sublimetext.com/): editor dominante, pestañas compactas, sidebar colapsable, palette, divisiones y panel inferior. El prototipo GUI debe validar esa composición, DPI, IME, accesibilidad y respuesta con nuestro motor. No se consultó código propietario de Sublime. La compatibilidad con paquetes/Python es otra iniciativa y sigue post-M5.
