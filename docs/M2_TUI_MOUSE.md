# Ratón y explorador modal de la TUI

Construcción local de2026-11-03, sobre el modelo compartido de documentos y vistas.
La captura de ratón se activa al entrar al terminal y se desactiva al restaurarlo.
Click, Mayús+click y arrastre lineal usan las posiciones visuales y límites de
grafemas del modelo; respetan tabs, CRLF y marcadores de caracteres invisibles.
La rueda desplaza la vista sin mover el caret ni cambiar el foco, pin o enlace.
Menús, pestañas, panel lateral y listas inferiores tienen objetivos de ratón
acotados y versionados con el frame. Un resize, cambio de vista, revisión o modal
invalida los objetivos y cancela un arrastre obsoleto.

El modal «Abrir carpeta / Abrir archivo» muestra directorio, ruta editable,
lista, botones y ayuda en español. Tab cambia entre ruta y lista, Ctrl+L enfoca la
ruta, Intro entra o abre, Alt+Arriba sube, F5 actualiza y Ctrl+Intro elige la carpeta
actual. Esc restaura el contexto anterior. El catálogo procede de un worker
cancelable y su Wakeup; pintar no enumera archivos. Las rutas se obtienen de la
identidad del nodo y conservan nombres Unicode nativos.

Se validaron seis pruebas unitarias de ratón y cuatro de navegador, además de
flujos reales en ConPTY propio: siete comprobaciones de ratón y cinco de navegador,
con salida0 y fuentes fixture intactas. Los informes originales están en
`work/mouse-conpty/20261103T223205Z-72168d85/report.json` y
`work/browser-conpty/20261103T223215Z-2f3223e9/report.json`.
No se probó un terminal físico, doble click de palabra, arrastre rectangular ni
autoscroll durante arrastre. Esos comportamientos no se anuncian como habilitados.
