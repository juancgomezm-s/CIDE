# Anexo M2-M4 — Alcance realista de comandos ABL

**Estado:** especificación; no pretende replicar íntegramente Progress Developer Studio (PDSOE). La integración con la AVM de OpenEdge se valida por versión y se mantiene opcional.

## Criterios

- El núcleo sigue siendo byte-preserving. Sin conversiones destructivas de encoding, BOM, CRLF/LF ni saltos finales.
- Las acciones de edición no arrancan Progress, LSP ni agentes.
- Las operaciones de muchas líneas producen un solo cambio lógico de undo.
- Los comandos, menús y atajos pasan por CommandRegistry y KeyMap tanto en TUI como en GUI.
- Las transformaciones con impacto semántico exigen parser/metadatos; si no existen, el comando permanece deshabilitado en lugar de simular éxito.
- Los asistentes destructivos requieren vista previa de diff y consentimiento.

## Frontera entre comandos universales y extensión ABL

Los comandos de duplicar/borrar/mover líneas, sangría, búsqueda, selección columna y el motor de Toggle Comment son **universales**. El perfil ABL aporta los delimitadores `/* ... */`, no un handler de edición especial. Los snippets se resuelven con un motor genérico y plantillas aportadas por la extensión.

Las funciones `Correct Case`, `Expand Keywords`, `Correct Indentation`, `Organize USING`, `Update Function Prototypes`, generadores OO y servicios de AVM/XREF/esquema son responsabilidad de la extensión interna opcional `progress.abl`, nunca de `jc-app`. Todavía **no existe** un cargador genérico ni un paquete operativo con ese ID: se define la separación y se implementará por etapas.

Contrato: [EXTENSION_ARCHITECTURE.md](EXTENSION_ARCHITECTURE.md).

## A — M2A: operaciones sin AVM

| Comando | Alcance | Estado |
|---|---|---|
| Toggle Comment | ABL: comentar/descomentar línea o selección con /* ... */ | Código incorporado, compilación pendiente |
| Toggle Block Comment | Delimitadores de bloque compatibles por lenguaje | Código incorporado, compilación pendiente |
| Indentar/desindentar | Tab y Shift+Tab, selección múltiple | Código incorporado, compilación pendiente |
| Duplicar/borrar línea | Ctrl+D y Ctrl+Shift+D | Código base y atajo ajustado |
| Move line | Subir/bajar conservando EOL | Código inicial |
| Add Procedure / Function / Trigger | Snippets parametrizados | Pendiente |
| Surround With | Plantillas IF/DO/REPEAT/FOR EACH sin inferir lógica de negocio | Pendiente |
| Unir/dividir líneas | Transformación textual segura | Pendiente |
| Eliminar líneas vacías o duplicadas | Selección explícita y vista previa | Opcional |

En ABL se adopta /* ... */ como estilo seguro y común. Progress documenta comentarios anidados. La compatibilidad con // no se presupone en proyectos 10.0B: será una configuración por versión/perfil cuando se pruebe en la AVM real.

## B — M3: tokenización y plantillas por versión

| Comando PDSOE | Alcance realista |
|---|---|
| Correct Case | Cambiar keywords reales sin modificar strings, comentarios, preprocesador o identificadores |
| Expand Keywords | Solo abreviaturas reconocidas según versión ABL |
| Correct Indentation | Analizar bloques ABL y aplicar formato tras mostrar diff |
| Add Constructor / Static Constructor / Destructor | Snippets para archivos .cls y versiones OO compatibles |
| Add Method / Property / Event | Plantillas OO con parámetros explícitos |
| Add Annotation | Plantillas de documentación y etiquetas ABLDoc |

Ctrl+Shift+F pertenece a Buscar en proyecto, no a Correct Case; Ctrl+Shift+O abre carpeta, no Organize USING. Ctrl+I puede equivaler a Tab en terminales clásicos y no debe asignarse ciegamente.

## C — M4: funciones que requieren semántica

| Comando PDSOE | Dependencia |
|---|---|
| Organize USING Statements | Resolver tipos y referencias antes de eliminar, ordenar o expandir imports |
| Update Function Prototypes | Analizar firmas reales y declaraciones |
| Constructors from Super Class | Jerarquía de clases y constructores disponibles |
| Override/Implement Members | Resolución de herencia/interfaces |
| Define Service Interface | Solo si hay contrato real del proyecto |

El compilador de ABL puede producir XREF / XREF-XML para referencias de RUN, includes, tablas, campos e índices. JC-IDE aprovechará dichos artefactos bajo demanda; las referencias dinámicas que el compilador no determina deben etiquetarse como inciertas.

## D — Opcionales o excluidos

- **Strip AppBuilder markup** no entra como acción automática: Progress advierte que después podría no volver a abrirse en AppBuilder. Solo utilidad posterior con diff, copia de seguridad y confirmación.
- Wizards visuales completos y generadores de arquitectura empresarial no serán dependencias del editor.
- No generar miembros OO para instalaciones o fuentes que no los soporten.

## Modo columna: requisito real, no decorativo

El modelo compartido necesita rangos rectangulares por línea y columna **visual**, clipboard rectangular, inserción/eliminación por fila y undo transaccional. Antes hay que resolver tabulaciones, Unicode de doble ancho, grafemas, columnas virtuales y bytes legacy.

Entradas objetivo: Alt+Shift+flechas cuando el terminal las reporte; comandos de Palette para fijar comienzo/fin cuando no las reporte; Alt+arrastrar en GUI. No declarar el modo operativo mientras no funcione la edición rectangular completa.

## Política de ligereza

- Sin AVM, Python, LSP, Git o indexador residentes al abrir.
- Tokenizador/plantillas por documento y bajo demanda, con caché acotada.
- No recorrer todo el workspace durante una operación del editor.
- Preview/undo para transformaciones masivas.
- Medir RAM y tiempo de arranque en M5; hasta entonces no activar CI remoto.

## Referencias oficiales

- [Progress: Use comments](https://docs.progress.com/bundle/openedge-developer-studio-help/page/Use-comments.html)
- [Progress: comentarios multilínea ABL](https://docs.progress.com/bundle/abl-reference/page/Multi-line-comments.html)
- [Progress: Source menu](https://docs.progress.com/bundle/openedge-developer-studio-help-122/page/Source-menu.html)
- [Progress: COMPILE y XREF](https://docs.progress.com/bundle/openedge-abl-reference-128/page/COMPILE-statement.html)

Los manuales recientes no certifican automáticamente las instalaciones 10.0B o 11.7 del usuario.

## Checkpoint posterior — selección rectangular acotada

El [rectángulo de una acción](M2_RECTANGULAR_SELECTION.md) ya conecta selección
visual TAB4/virtual, clipboard interno y undo único a comandos/menu/Palette/TUI.
Su repertorio de una celda es explícito; grafemas/ancho doble y Unicode general
se rechazan y siguen pendientes. No declarar cerrado el requisito general de
modo columna ni clipboard OS. 310 pruebas distintas en cierre selectivo,
fmt/Clippy/release locales. Próximo bloque activo: unir/dividir líneas genéricos.
