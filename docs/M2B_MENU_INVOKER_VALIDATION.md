# Seguimiento del invocador Find/Replace — 2026-11-03

Base `171af05df46257ae3f0d24604716e3a0552c0a29`, árbol limpio al comenzar.
Este seguimiento cierra evidencia pendiente del contrato de menús; no admite
nuevos gestos de producto ni cambia sus semánticas para acomodar el arnés.

## Auditoría de admisión previa a construcción

Se revisaron README, decisiones, PLAN, AGENTS, el inventario GUI y contratos de
interacción/selectores. La búsqueda transversal de los documentos no encontró
admisión específica de los cuatro gestos siguientes:

| Candidato | Evidencia vigente | Decisión |
|---|---|---|
| Mouse horizontal | PLAN §«Menús nativos y gestos de fuente», líneas578–583, lo conserva pendiente. Contrato M2B_NATIVE_INTERACTION líneas26–31 excluye scroll horizontal con modificadores. M2_INVISIBLES_MULTILINE describe recorte de celdas de proyección, no admite un gesto nuevo | No construir: falta decisión de alcance y semántica |
| Autoscroll | Contrato M2B_NATIVE_INTERACTION líneas19–24 limita captura al área visible; AGENTS líneas478–480 dice «No autoscroll or word selection» | No construir: contrato actual lo excluye |
| Selección por palabra | Mismo contrato, línea24, excluye word-selection por doble click; las rutas de dobleclick de Explorer/Problems pertenecen a sus controles | No construir: falta contrato de fronteras/Unicode y gesto |
| Mouse en campos | M2B_NATIVE_INTERACTION línea24 excluye drag del selector; M2B_SELECTOR_INPUT líneas96–98 conserva colocación del caret por mouse como hueco independiente | No construir: falta alcance por tipo de campo y mapeo de display |

PLAN §M2B, línea130, establece aceptación general de mouse, clipboard, IME y
accesibilidad. No define la semántica ni selecciona estos incrementos. AGENTS
§Development, líneas19–27, autoriza continuidad dentro del plan y aclara que no
autoriza expansión. Las menciones «pendiente/no implementado» no se transforman
en nuevas órdenes ni se modifican retrospectivamente para justificar código.

Sí está admitido verificar paleta→macro de menú con Find subyacente: D49 en
DECISIONES líneas530–532 promete preservar el draft invocador; AGENTS475–476 lo
mantiene y reconoce la prueba ausente; M2B_NATIVE_INTERACTION95–96 registra el
caso separado no ejecutado. Por eso el único bloque activado es esta validación
integrada de comportamiento existente, con Find y Replace, no nueva funcionalidad.

## Alcance del recorrido

Las seis macros MenuFile/Edit/Search/Project/Tools/View desde paleta deben abrir
el HMENU y conservar draft, caret y campo activo de Find/Replace al cancelar.
Comprobar repetición, continuar edición de campo, una acción de menú permitida,
edición fuente deshabilitada y reentrada con cambio de draft; preservar documentos,
bytes/BOM/encoding/EOL, revisión, undo, vistas/split/pin y disco. La elección y el
clipboard del arnés son transportes sintéticos; solo se controla el HWND propio.

Se construye/cablea todo el recorrido y su sección interna aislada antes de
ejecutarlo. No repetir la sección previa de13 checks ni suites históricas sin
impacto.

## Resultado observado

Construcción completa antes de la ventana serial: worker existente en un único
archivo exclusivo de arnés; integrador en routing interno/CLI/script, evidencia
y commit. **Un recorrido integrado nuevo**, `native_menu_palette_find_invoker_roundtrip`,
aprobado a la primera. Ocho checks de la sección: ese caso y siete comunes de
arranque/pintura. No son36 pruebas independientes ni se suman como una suite
global con el cierre anterior.

Tres estados: Find, Replace con reemplazo activo y Replace con query activo.
Cada uno recorre las seis macros mediante la entrada/aceptación real de la paleta,
con dos cancelaciones por macro (36 cancelaciones base), sin llamar directamente
a execute(Menu*) para fingir su invocador. Se comprueba el traslado temporal del
draft a la paleta, su propiedad al restituirse y la cola de elección consumida.
Texto multibyte/emoji/CRLF, ambos carets bytes, campo activo, modo y generación
se conservan exactamente tras cada vuelta. WM_CHAR y Backspace posteriores
editan/restauran el campo y avanzan su generación, sin tocar undo de fuente.

ViewToggleInvisibles a través de MenuView se ejecuta y revierte conservando el
draft; EditDuplicate a través de MenuEdit permanece rechazado sin cerrar Find
ni editar fuente oculta. Ocho reentradas del mismo transporte HMENU sobre el
invocador ya restituido cambian texto/caret/campo antes de elegir View: la acción
tardía se descarta y no cambia la preferencia. Son inyecciones controladas de
transporte, no elección física de TrackPopupMenuEx ni un hook nuevo de producto.

Source13B ASCII nunca se edita. Durante los flujos se conservan bytes, identidad,
revisión, encoding, dirty/undo/redo y descripción de vistas/split/pin; al cerrar,
catálogo, documento/disco, consulta inicial y preferencia invisibles restaurados,
scan cancelado y sin intención de popup reutilizable. No se repite la matriz de
BOM/encodings fuente del bloque previo; Unicode/CRLF aquí son los drafts.
Metadata Git propia conservada; un WM_PAINT, cero errores. Los16ms del arnés
no miden latencia física ni RAM del editor.

Clippy `-p jc-gui --all-targets -D warnings`, release GUI `--bins` y fmt aprobados
offline en Rust1.85.1/MSVC x64. Tres invocaciones CLI rechazan falta de rutas
propias, flag duplicado y mezcla de secciones con exit2. **Cero ejecuciones de
pruebas unitarias**, sin suites runtime históricas ni perfiles repetidos.
Todos los checks pasaron a la primera, sin corrección productiva ni del oráculo.
Recorrido reproducible con el GUI local construido:
`python scripts/smoke-native-gui.py --section menu-invoker`.

[Reporte HWND](evidence/m2b-menu-invoker-2026-11-03-native-report.json),
[logs/hashes/alcance](evidence/m2b-menu-invoker-2026-11-03-metadata.json),
[rechazos CLI](evidence/m2b-menu-invoker-2026-11-03-cli.json).
Los hashes confirman sin cambios los handlers de menú, selección de paleta,
SearchPanel, campos, pointer, edición y pintura frente a171af05. Cambian solo
arnés y routing interno de automatización; sin dependencia ni nueva función de
producto. No se genera otro paquete por esta evidencia.

## Decisión necesaria para más construcción

El seguimiento cierra el recorrido pendiente admitido. **No se activa construcción
de mouse horizontal/autoscroll/word-selection/mouse en campos**: elegir qué conjunto
pertenece al siguiente incremento y aprobar su contrato de gestos, coordenadas,
límites y cancelación es la decisión pendiente. La aceptación general de M2B y
la lista de limitaciones no sustituyen esa admisión. No se añade alcance al plan
como justificación retrospectiva ni se inicia M3/proveedores por inferencia.

Permanecen abiertas las puertas físicas/globales/Unicode/clipboard externo/IME/
DPI/accesibilidad/symlink/notices. Sin clipboard privado, permisos/credenciales/
seguridad nuevos, instalaciones/dependencias/proveedores/AVM/terminal/regex,
CI/push/publicación, Qwen/GPU ni actividad sobre RAG.
