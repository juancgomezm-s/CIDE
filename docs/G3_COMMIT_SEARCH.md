# Búsqueda de commits — 2026-11-06

GUI y TUI usan jc_git::commit_search, independiente del editor, sobre un inicio
fijado. Consulta literal por mensaje completo, autor/correo o prefijo SHA; Todos
combina esos campos. Hasta 256 bytes UTF-8, comparación por minúsculas Unicode
simples; no es regex ni case-fold completo. No une fragmentos separados por bytes
UTF-8 inválidos. La fuente original y sus bytes permanecen intactos.

Recorre únicamente el primer padre, hasta 128 commits y 24 coincidencias por página.
El cursor liga raíz, directorio Git, inicio, texto y campo; no puede reutilizarse
con otra consulta. Una página vacía puede tener continuación. Se distinguen frontera
shallow y metadatos recortados. Se leen/verifican objetos commit con cuota de 64 KiB
por objeto y presupuesto acumulado; no árboles, blobs ni worktree para buscar.
No hay índice permanente, grafo completo, red ni escrituras Git. Las cuotas y el
timeout del controlador son cooperativos, no garantías de RSS o interrupción dura.

El único GitRevisionController y su worker existente atienden esta consulta.
Solo poll acepta resultados de raíz/generación/consulta vigentes. Preparar o pintar
no inicia lecturas. Se conserva una página al abrir una revisión perteneciente a
ella, usando la comparación histórica existente; editar texto/campo, cancelar o
cambiar raíz invalida los resultados. No se clonan documentos ni historial undo.

GUI: Buscar commits o Ctrl+F dentro del espacio Git; Intro ejecuta, F2 cambia campo,
Tab alterna consulta/resultados y N pide la siguiente página. Clic/Intro abre archivos
de la revisión; Esc/B regresa. Campo acotado con movimiento por grafemas y pegado
fuera del préstamo del modelo, validado de nuevo con token al recibir el texto.
TUI: F desde el lateral Git; Intro ejecuta/abre, F2 campo, Tab foco, N siguiente,
R/F5 relee el inicio fijado y Esc/B vuelve. El cursor acompaña consultas anchas.
Ambas interfaces conservan el tema y textos en español; los mensajes del repositorio
se muestran sin traducir. Al salir de búsqueda se retorna al historial/estado general.

Las pruebas dirigidas cubren mensaje/correo/SHA, Unicode/bytes inválidos, cuotas,
paginación y continuación vacía, cancelación, cursor y objetos ausentes; entrega
solo en poll y rechazo de resultados ajenos. Los flujos GUI/TUI prueban búsqueda,
apertura, regreso y edición sin cambiar buffer sucio, vistas ni fuentes.
El JSON de evidencia registra los resultados exactos. No se toma el escritorio,
portapapeles personal ni recuperación personal. CI permanece dormido. IME excluido,
Narrador aplazado y aceptación física/global M2 sin ampliar.

Formato, Clippy selectivo all-targets y release workspace offline aprobaron.
[Evidencia](evidence/g3-commit-search-2026-11-06.json).
