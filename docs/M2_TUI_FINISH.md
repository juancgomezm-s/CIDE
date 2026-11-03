# Continuación hasta cerrar los pendientes TUI

Instrucción de Juan del2026-11-03: trabajar en bloques amplios, validar al cierre
y continuar con el siguiente sin pedir otro «continúa». El cierre completo se
evalúa por comportamiento/evidencia; las puertas externas no se declaran cumplidas.

| Bloque | Estado y salida |
|---|---|
| Regex real Search Plus | Integrado y validado localmente: [contrato](M2_REGEX_SEARCH.md). Reemplazo literal, cuotas y limitaciones explícitas. |
| Comparación de dos abiertos sin Git | Integrada TUI y validada: [contrato](M2_OPEN_DOCUMENT_DIFF.md). GUI pendiente independiente. |
| Capacidades internas y navegación textual | Integrado/validado: registro interno, proveedor opcional y símbolos versionados, sin segundo buffer; inferencia lexical separada del compilador. [Evidencia](M2_SOURCE_SERVICES.md). |
| Includes/RUN y completado | Includes/RUN léxicos y candidatos confinados integrados; dinámicos no resueltos. Completado textual local integrado; semántico permanece M4/servicio real. |
| Plantillas/snippets M2A | Integrado y validado: motor universal, siete plantillas del proveedor opcional, parámetros/preview/aceptación y undo único. [Evidencia](M2_SNIPPETS.md). |
| Auditoría final de teclado/flujo | Revisar comandos habilitados, foco, guardar/cerrar y resultados; construir huecos reales antes de la ventana final de pruebas. |
| Aceptación física/global | RAM/arranque/latencia real, teclado/terminales, Unicode general/IME y clipboard OS conservan evidencia y límites; no se cierran con modelo o ConPTY. |

Progress Check/Compile/Run/Schema exige perfil/herramientas/versión reales y
permanece deshabilitado si no existe proveedor validado. Git con escritura/CI
es M5; runtime externo de plugins post-M5. El shell embebido no se activa por
llamar «terminada» a la TUI: el contrato actual usa la terminal anfitriona.
