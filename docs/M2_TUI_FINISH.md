# Continuación hasta cerrar los pendientes TUI

Instrucción de Juan del2026-11-03: trabajar en bloques amplios, validar al cierre
y continuar con el siguiente sin pedir otro «continúa». El cierre completo se
evalúa por comportamiento/evidencia; las puertas externas no se declaran cumplidas.

| Bloque | Estado y salida |
|---|---|
| Regex real Search Plus | Integrado y validado localmente: [contrato](M2_REGEX_SEARCH.md). Reemplazo literal, cuotas y limitaciones explícitas. |
| Comparación de dos abiertos sin Git | Integrada TUI y validada: [contrato](M2_OPEN_DOCUMENT_DIFF.md). GUI pendiente independiente. |
| Capacidades internas y navegación textual | Siguiente bloque activo: diseñar API/ciclo de vida genérico y proveedor interno opcional; símbolos/ubicaciones ligadas a revisión, sin segundo buffer. La inferencia local debe etiquetarse separada de evidencia del compilador. |
| Includes/RUN y completado | Integrar con el proveedor, candidatos explícitos y raíces autorizadas; referencias dinámicas no se resuelven inventando rutas. Completado local y semántico son capacidades distintas. |
| Plantillas/snippets M2A | Motor universal/transacción y contenido ABL del proveedor opcional; no casos especiales de lenguaje en jc-app. |
| Auditoría final de teclado/flujo | Revisar comandos habilitados, foco, guardar/cerrar y resultados; construir huecos reales antes de la ventana final de pruebas. |
| Aceptación física/global | RAM/arranque/latencia real, teclado/terminales, Unicode general/IME y clipboard OS conservan evidencia y límites; no se cierran con modelo o ConPTY. |

Progress Check/Compile/Run/Schema exige perfil/herramientas/versión reales y
permanece deshabilitado si no existe proveedor validado. Git con escritura/CI
es M5; runtime externo de plugins post-M5. El shell embebido no se activa por
llamar «terminada» a la TUI: el contrato actual usa la terminal anfitriona.
