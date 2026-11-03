# Capacidades locales de fuente — TUI implementada, 2026-11-03

> Actualización 2026-11-03: [GUI/TUI en español](M2B_GUI_WORKFLOWS_ES.md)
> incorpora comparación, fuente opcional y plantillas en GUI; color/menús Alt,
> ayuda lateral y Markdown en TUI, Markdown también en GUI. F10 abre menús,
> F6 cambia panel y Ctrl+Alt+M alterna Markdown. Las matrices, cifras o pendientes
> anteriores de este documento se conservan como evidencia histórica.


Construido sobre bd83fa0, conservando resaltado, regex, comparación dual y F1/F2/F3.
El contrato se escribió antes de implementar; la evidencia de cierre se registra
abajo. Es un módulo interno opcional operativo de fuente, no loader externo.

## Alcance admitido

Registro interno acotado de capacidades por lenguaje/ID namespaced. API común
de símbolos, referencias textuales y completado local; sin runtime externo.
Proveedor opcional progress.abl en jc-progress, habilitado por feature desde TUI;
sin casos ABL en jc-app y sin iniciar AVM por abrir/analizar. Proveedor universal
de palabras locales como fallback de completado. No completado semántico, XREF,
resolución dinámica o certificación de gramática/versión OpenEdge.

Los símbolos ABL se infieren léxicamente (Procedure/Function/Class/Method),
excluyendo comentarios/strings/preprocesador mediante el lexer compartido.
Referencias incluyen rutas estáticas de include/RUN; las dinámicas/inciertas se
presentan como no resueltas, sin inventar destino. Rutas relativas candidatas en
directorio del fuente/raíz de proyecto, selección explícita si hay ambigüedad y
apertura por la API confinada existente. No escaneo permanente de PROPATH.

## Identidad y entrega

SourceRegistry admite hasta16 descriptores internos, IDs únicos y capacidades
opcionales. No es un loader de plugins ni sustituye todavía los Progress* fijos.
Comandos universales resuelven la capacidad por perfil; un proveedor ausente
deshabilita su función. El frontend solo adapta elección/foco/presentación.

SourceController conserva un trabajo en vuelo y el pendiente más reciente de
solo metadata. Captura snapshot de hasta2MiB mediante JobSystem; para completar,
solo0..caret. Fuente, encoding, revisión, ruta, ViewId y generación se comprueban
al entregar/activar. Completar exige también caret/selección originales. Los
workers reciben snapshot inmutable y cancelación, nunca Workbench ni disco.

Resultados SourceItem tipados, hasta64/64KiB según F3. Distinguen navegar en el
documento, referencia relativa, referencia no resuelta y propuesta de reemplazo.
Host valida rangos y capacidad de acción antes de aceptarlos. Completado solo
se aplica por aceptación explícita, revisión esperada y una transacción estricta
de encoding; no desde poll. No copia de documento ni historial en el proveedor.

## TUI y cierre esperado

View Symbols y referencias usan lista acotada con teclado/refresh/cancelación y
destinos versionados. Completado usa selección explícita; typing/pegado del
selector no edita fuente inadvertidamente. Menú/paleta son fallback para teclas
que el terminal no entregue. Presentaciones preparan solo filas visibles; render
no inicia/acepta trabajo ni resuelve rutas. Editar/cambiar contexto marca stale;
no se activa un indexador automático.

Construir registro/controller, proveedor, resolución/aplicación y TUI completos
antes de pruebas. Cierre dirigido cubrirá fuentes UTF8/legacy, comentarios y
strings, rangos/CRLF, cancelación/generaciones, edición/revisión/caret stale,
ambigüedad/escape de raíz, dirty/pin/undo, completado explícito y ConPTY propio.
AVM, parser exhaustivo, semántica, GUI de estas capacidades y aceptación física/
global siguen abiertos hasta evidencia específica. Plantillas/snippets seguirán
en un bloque amplio posterior sobre esta frontera opcional.

## Uso y semántica implementados

View/Project Symbols abre la lista léxica ABL; References in Document enumera
includes y RUN. Up/Down/Home/End eligen, Enter navega, Ctrl+Enter abre referencia
de archivo al lado; F5 refresca y Escape cancela/vuelve al editor. Los comandos
Accept/Refresh/Cancel/Open Source Item to Side también están en menú/paleta.
Una lista stale no permite navegar ni aplicar. Cambiar de documento/vista/raíz,
encoding o revisión invalida; no se reindexa automáticamente al teclear.

Edit/Palette Complete Local Word (Ctrl+Space si el terminal lo entrega) muestra
palabras textuales anteriores al caret: case-sensitive, sin gramática/semántica,
incluyendo palabras de comentarios/strings. No consulta el resto del proyecto ni
el sufijo posterior al caret. Requiere caret sin selección/rectángulo. El selector
bloquea entrada/pegado fuente; Enter aplica una propuesta, Escape cancela. Caret,
anchor, revisión, encoding y rango de prefijo se revalidan; hasta512 bytes fuente
reemplazables y4096 UTF8 de propuesta. Un undo restaura exactamente los bytes.

El proveedor progress.abl no afirma resolución del compilador: omite/infiere un
subconjunto Procedure/Function/Class/Method, conserva rangos de nombres calificados
y tokens fragmentados; no interpreta gramática completa, abreviaturas o expansión
de includes. Declaraciones con sintaxis/comentarios interpuestos no soportados
pueden omitirse. Si el lexer indica límite o se exceden64 resultados, el estado
es Limited sin lista parcial aplicable. Los modelos temporales del proveedor
también son acotados (64 símbolos y64 referencias, nombres/rutas de hasta512
bytes fuente); la cuota F3 de64KiB contabiliza resultados, no RAM privada completa.

RUN VALUE/IN/ON/ASYNCHRONOUS y formas escapadas/dinámicas no se resuelven como
archivo local. Un RUN interno sin contexto puede apuntar a una declaración local
coincidente, etiquetada lexical. Otros nombres sin extensión quedan sin resolver;
no se emula PROPATH, r-code libraries, servicios remotos ni semántica de handles.
Las rutas estáticas se buscan solo en directorio del fuente y raíz del proyecto.
Dos coincidencias muestran candidatos relativos distinguibles; aceptar revalida
origen/candidato y usa open_project_file por handle. No se hereda el encoding del
fuente en el destino: un abierto conserva el suyo, un cerrado se detecta o solicita
interpretación explícita. Dirty/pin/undo siguen en el único Workbench.

Referencias primarias consultadas: [RUN](https://docs.progress.com/bundle/abl-reference/page/RUN-statement.html),
[PROCEDURE](https://docs.progress.com/bundle/abl-reference/page/PROCEDURE-statement.html).
Estos manuales y las inferencias locales no acreditan compilación en10.0B/11.7.

## Cierre local

40 casos distintos aprobados: siete del host, cinco del proveedor, cinco TUI,
ocho jobs/F3 y15 interacción.17 nuevos. El bloque completo se cableó antes de
pruebas; después se repitieron solo conjuntos afectados por correcciones. Fmt,
Clippy workspace/all-targets y build release workspace bins/examples aprobaron
offline. Sin dependencias externas nuevas: la feature source-services añade
dependencias de ruta opcionales de jc-progress a jc-app/languages/text y TUI la
activa estáticamente; el puente AVM por defecto sigue sin esas dependencias.

La validación detectó espacios Plain tomados como nombres: se filtraron palabras
reales del lexer. Una regresión adicional identificó rangos parciales de nombres
largos de métodos: ahora se reúnen tramos contiguos y se evita usar return types
como nombres. Un lint de fixture se corrigió. Se conservan logs iniciales y finales.
En ConPTY, la primera expectativa buscaba Ln/Col ocultos por una ruta larga; el
arnés usa la posición real del cursor VT, sin cambiar producto para ese oráculo.

ConPTY final: cuatro flujos,34169 bytes VT, salida0 y tres fixtures intactos:
símbolos/navegación, completar+undo, include confinado y RUN al lado sin ejecutar.
El caso adicional TUI comprobó documento dirty/pin/code page Windows1252 propia
frente a un fuente UTF8, identidad y undo sin tocar disco. No nueva corrida GUI,
AVM, clipboard OS, pantalla física ni aceptación global de rendimiento.

Metadatos, hashes, logs y paquete verificado se retienen bajo
`docs/evidence/m2-source-services-2026-11-03-*`. Las capacidades Source* gráficas
quedan deshabilitadas hasta tener handlers; no se afirma paridad GUI ni extensión
dinámica. Los Progress* de ejecución continúan deuda de registro/configuración.

Siguiente bloque TUI: motor universal de plantillas/snippets y contenido ABL
opcional, con parámetros/preview/aceptación, límites y una transacción. No añadir
casos de lenguaje en jc-app ni afirmar que plantillas hayan compilado en AVM.

La admisión Source exige el stamp esperado y mantiene solo metadata bajo presión
transitoria compartida. Un caso adicional verifica60 bytes reservados frente a
un pedido de64, espera sin captura y entrega posterior al liberar el primer job.

[Metadatos y hashes](evidence/m2-source-services-2026-11-03-metadata.json), [ConPTY final](evidence/m2-source-services-2026-11-03-conpty-3.json), [paquete verificado](evidence/m2-source-services-2026-11-03-package.json). ZIP local de desarrollo: `work\packages\CIDE-windows-dev-source-services-2026-11-03-final.zip`, 5450923 bytes/76 miembros; SHA256 `f71f5a082ee88ed1e0389776d366fc6da54a938aece2d6d5b48c2254b6a4bb44`. Permanecen dos avisos GNU pendientes, sin certificación de distribución.
