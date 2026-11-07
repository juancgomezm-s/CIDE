# Markdown: imágenes, sangrías y enlaces — 2026-11-06

**Ver → Vista previa de Markdown** conserva su check y solo se habilita en
Markdown. Esc vuelve a la fuente. La GUI dibuja PNG, JPEG, BMP y el primer
fotograma de GIF. Las imágenes relativas se cargan en segundo plano desde la
carpeta del documento o el proyecto abierto. Una imagen web muestra **pulsa
para cargar**; abrir o desplazar el documento no inicia descargas sin esa acción.
Las referencias no admitidas conservan descripción y diagnóstico visible.

Listas ordenadas/no ordenadas y tareas conservan niveles por espacios/TAB4 y
sangría colgante al envolver. Las etiquetas de enlaces se muestran sin repetir
el destino. Clic o Tab/Intro activa; **Ctrl+clic/Ctrl+Intro** abre el archivo
local al lado. Una dirección HTTP/HTTPS abre el navegador registrado. Un
fragmento navega al primer encabezado coincidente; los fragmentos de otro archivo
Markdown se resuelven tras abrirlo. La TUI conserva texto alternativo y permite
Tab/Mayús+Tab e Intro/Ctrl+Intro; las imágenes web se pueden abrir explícitamente
en el navegador. El terminal no recibe mapas de píxeles.

No se cambia el documento ni su undo al preparar, dibujar, descargar o saltar a
un ancla dentro de la vista. Abrir otro archivo usa Workbench y reutiliza un
buffer ya abierto sin recargar cambios pendientes. Se conservan byte/codec/EOL
y validación de revisión/raíz/vista. Los hits usan geometría DPI, origen del
Explorador y token de la escena; solo se publican tras pintar correctamente.

## Diseño y límites

El mismo proyector Markdown emite tipo de bloque, sangrías y destinos tipados
por referencia compartida. No crea un DOM, otro parser global ni un documento
editable. El texto alternativo queda disponible mientras carga una imagen.
El resolver acepta rutas relativas con espacios/percent-encoding estricto y
fragmentos; no admite rutas absolutas, UNC, esquemas ejecutables ni credenciales
web. Aperturas locales quedan confinadas al proyecto o carpeta del documento.
No se ofrece CommonMark completo, SVG, scripts, animación, autorrotación ni
redirecciones hacia HTTP desde HTTPS.

Anclas: recorrido cooperativo de 64 líneas/256 KiB por entrega, hasta 65.536
líneas/4 MiB totales y 16 KiB por línea. Se omiten fences, se verifica identidad
antes de leer, y no se mantiene un índice permanente. Un límite aparece como
resultado explícito; no mueve el caret del fuente.

Imágenes: un único worker de aplicación, cola última solicitud, una entrega y
aceptación solo en poll. Usamos el lector confinado jc-fs ya existente y códecs
nativos GDI+. No se lee ni decodifica imagen en prepare/draw. Hasta 16 recursos
por página y 16 destinos web admitidos por vista, 8 MiB codificados por archivo,
4096 por dimensión, 4M píxeles y 16 MiB decodificados por caché/entrega. Caché y
entrega pueden coexistir hasta 32 MiB lógicos antes de compartir referencias;
buffers fuente, frames y memoria nativa se contabilizan aparte. No es un límite
RSS del proceso. Dimensiones de hasta 64 recursos se recuerdan por contexto para
que expulsar píxeles no cambie la geometría y vuelva a solicitar la misma página.
Grupos de imágenes se ajustan al espacio visible, con proporciones conservadas.
Cerrar la vista cancela sin esperar; el worker termina al cerrar la aplicación.

HTTP/HTTPS se descarga con WinHTTP directo, sin proxy, cookies ni autenticación
ambiental. Redirecciones manuales acotadas a tres; timeout cooperativo por petición,
además de timeouts nativos. No hay garantía de plazo duro durante una llamada al
sistema. [Opciones nativas consultadas](https://learn.microsoft.com/en-us/windows/win32/winhttp/option-flags).

## Evidencia

33 casos dirigidos distintos aprobaron por suites y cierres afectados; 20 nuevos.
Incluyen sangrías/wrap, análisis de URI, anclas, enlaces obsoletos, apertura lateral,
PNG/BMP/alfa, imágenes múltiples, viewport compacto, reflow sin ciclo de recarga,
render GDI y HTTP contra fixtures locales. El entorno restringido devolvió
12029 en loopback; los tres casos HTTP finales aprobaron contra 127.0.0.1 en una
corrida local aislada sin esa restricción. No se probó Internet ni un handshake TLS.
No se lanzó navegador en pruebas. Formato, Clippy selectivo all-targets y release workspace offline aprobaron.
No se tomó el escritorio ni se usó portapapeles/recuperación personal. La aceptación
física pendiente de Computer Use del bloque anterior no se reabre ni se da por aprobada.

[Imagen del pintor nativo](evidence/markdown-media-native.png), fixture sintético
GDI sin ventana física. [Resultados](evidence/markdown-media-2026-11-06.json).
Binarios: target/markdown-media-next/release. Rutas previas conservadas; CI dormido.
