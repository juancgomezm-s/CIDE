# Clipboard Windows TUI — implementado, 2026-11-03

Construido sobre8a904ba y validado al cierre del bloque. Sin dependencia
externa nueva: extraer el adaptador existente GUI a un crate de plataforma sin
dependencia de UI o núcleo. GUI conserva su HWND; TUI crea bajo demanda un HWND
message-only propio, invisible, en un hilo Win32 dedicado bajo demanda, sin
proceso auxiliar. GUI usa su HWND existente directamente.

Solo Copy/Cut/Paste explícitos usan CF_UNICODETEXT; no sondeo, historial, lectura
de disponibilidad ni acceso al iniciar/dibujar. Headless TuiApp mantiene transporte
interno; el loop terminal Windows conecta transporte nativo sin acceder al OS
hasta la primera acción. Pruebas usan transporte inyectado en memoria y no leen
ni reemplazan el clipboard personal.

Exportar selección por rangos, publicar antes de cortar, revalidar destino F1 tras
handoff. Paste captura revisión/vista/encoding/caret/anchor y hace una transacción
estricta; no normaliza EOL ni adivina code pages. Error deja fuente/undo intactos.
Rectángulos conservan clipboard tipado interno; no inventar formato rectangular
interoperable. Sin fallback silencioso a texto interno al fallar un read del OS.

CtrlV también pega en el campo del overlay superior, snippets y campos Find/
Replace; no en visores de solo lectura. Cuotas de destino se comprueban antes de
retener texto y se revalidan al editar. El adaptador compartido conserva UTF16
estricto, NUL, cuotas2MiB, RAII y fallo inmediato por contención. Delayed rendering
externo no tiene deadline garantizado; EmptyClipboard/SetClipboardData no es un
rollback atómico del clipboard ajeno.

Referencias ABI/propiedad: [operaciones](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations),
[CreateWindowExW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createwindowexw).
La verificación con memoria y HWND propio no certifica interoperabilidad con otras
aplicaciones, terminales ni clipboard real; esa aceptación sigue física.

## Clipboard Windows TUI — cierre local 2026-11-03

Adaptador CF_UNICODETEXT de la GUI extraído a jc-platform (sin dependencias
externas); ambos frontends comparten implementación. TUI conecta transporte en
el loop real y crea HWND propio invisible bajo demanda, solo por Copy/Cut/Paste.
Headless conserva clipboard interno; pruebas inyectan memoria, no el clipboard
personal. Cortar después de publicar, pegar por destino/revisión y un undo;
cuotas/encoding estricto, campos del overlay superior y rectángulos internos.

32 casos Rust distintos aprobaron: cinco de plataforma (cuatro migrados y
uno nuevo de HWND propio), siete TUI nuevos y regresiones de clipboard/F1/registro.
Fmt, Clippy workspace/all-targets y release bins/examples aprobaron offline.
GUI HWND propio:13 checks selector-input, GDI sin error, fixture y metadata Git
intactos, clipboard sintético. ConPTY TUI: tres flujos,27436 bytes VT/salida0,
fixture intacto. No lectura/escritura de clipboard personal ni certificación OS.
Un fixture usó un nombre de método inexistente y un campo tenía visibilidad más
amplia que su trait; se corrigieron antes del cierre, logs retenidos.

Layout Unicode ya implementado con las bibliotecas autorizadas;
[contrato y evidencia](M2_UNICODE_LAYOUT.md).
Esa autorización sustituye la restricción previa para estas dos dependencias;
no pedirla de nuevo. Sin instalación de programas, CI ni AVM.

[Metadatos/hashes/paquete](evidence/m2-tui-clipboard-2026-11-03-metadata.json). ZIP local de desarrollo `work/packages/CIDE-windows-dev-tui-clipboard-2026-11-03.zip`, SHA256 `dcb18a76d27a162cb945ae25ff032a5b4e2d12d8849af88c535267b47f227205`; CRC/manifiesto verificados. Dos avisos GNU pendientes, sin certificación de distribución.

## Propietario que responde estando inactivo — cierre posterior

La revisión Unicode detectó que esperar entrada TUI no bombea mensajes Win32.
Se sustituyó el owner en el hilo principal por un actor std lazy: un hilo, HWND
message-only creado/destruido en él, GetMessageW y un solo buzón acotado. Sólo
texto poseído <=2MiB y respuestas cruzan el buzón; nunca Document, FileSession ni
callbacks al editor. Preflight inválido no crea hilo/ventana. Readiness se publica
tras crear la cola; cierre marca el buzón antes de liberar respuestas, también
por unwind. Post fallido devuelve error; no fallback silencioso ni sondeo.
Drop despierta y reúne el hilo; si falla PostThreadMessage lo separa para no
bloquear esperando una cola imposible de despertar. No se fuerza terminación.

La prueba del HWND propio verifica distinto hilo/mismo proceso, invisibilidad,
cuatro WM_NULL con SendMessageTimeout mientras el llamador no bombea y destrucción
al cerrar. Pasaron cinco pruebas de plataforma y siete TUI de clipboard tras el
cambio; sin OpenClipboard ni datos personales en esta prueba de propietario.
GetMessage atiende mensajes enviados mientras espera; la cola debe existir antes
de PostThreadMessage: [GetMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getmessagew),
[PostThreadMessageW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-postthreadmessagew),
[MSG](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msg).
Delayed rendering externo aún puede bloquear una transferencia sin deadline
duro; este hilo no certifica cancelación de llamadas OS ni interoperabilidad real.
Evidencia posterior en `m2-unicode-layout-2026-11-03-*`.
