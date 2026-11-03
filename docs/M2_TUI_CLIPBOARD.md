# Clipboard Windows TUI — implementado, 2026-11-03

Construido sobre8a904ba y validado al cierre del bloque. Sin dependencia
externa nueva: extraer el adaptador existente GUI a un crate de plataforma sin
dependencia de UI o núcleo. GUI conserva su HWND; TUI crea bajo demanda un HWND
message-only propio, invisible, en el hilo UI y sin proceso auxiliar.

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

[Contrato y límites](M2_TUI_CLIPBOARD.md). Sigue layout Unicode general: Juan
autorizó unicode-segmentation=1.13.3 y unicode-width=0.2.2; [admisión](M2_UNICODE_LAYOUT_ADMISSION.md).
Esa autorización sustituye la restricción previa para estas dos dependencias;
no pedirla de nuevo. Sin instalación de programas, CI ni AVM.

[Metadatos/hashes/paquete](evidence/m2-tui-clipboard-2026-11-03-metadata.json). ZIP local de desarrollo `work/packages/CIDE-windows-dev-tui-clipboard-2026-11-03.zip`, SHA256 `dcb18a76d27a162cb945ae25ff032a5b4e2d12d8849af88c535267b47f227205`; CRC/manifiesto verificados. Dos avisos GNU pendientes, sin certificación de distribución.
