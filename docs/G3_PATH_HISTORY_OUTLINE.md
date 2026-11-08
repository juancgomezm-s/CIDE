# Historial por archivo y control del Esquema — 2026-11-06

Incremento de consulta G3 para GUI/TUI. Se conserva el diseño oscuro aprobado y
el núcleo Rust independiente; no habilita escrituras Git ni amplía M2 físico.

## Núcleo reutilizable

`jc_git::path_history::read_path_history(root, pinned_start, path, continuation,
limits, cancelled)` entrega `PathHistoryPage`: identidad de repositorio, inicio
fijado, ruta Git opaca, revisiones coincidentes, cursor siguiente, número de
commits examinados e indicadores shallow/recorte de metadatos. Cada entrada
incluye commit, primer padre, resumen/autor acotados, clase de cambio y
ruta de esa revisión. Un renombrado también conserva la ruta del padre.

Se comparan ID y modo de la entrada contra el primer padre, incluyendo creación,
eliminación y cambios de modo. Un commit raíz se compara contra ausencia.
La búsqueda sigue el primer padre y reconoce renombrados exactos con un
único blob del mismo modo en ambos árboles y una ruta anterior que
desaparece. Copias, coincidencias múltiples y cambios de contenido o modo
se conservan como altas. La detección comparte la cuota del recorrido.
Listar solo lee metadatos de Git. La comparación abre las dos rutas de
la revisión y reutiliza el modelo de diff existente.

Cada página admite hasta 24 coincidencias y 128 commits examinados. Una página
sin coincidencias puede tener continuación y no se presenta como fin. El cursor
tipado, de campos privados, liga raíz/gitdir, inicio, ruta inicial, ruta
rastreada y próximo commit;
se rechaza su uso con otra consulta. HEAD puede avanzar sin cambiar esta consulta.
Al llegar a una frontera shallow declarada no se inventa el cambio que requeriría
el padre ausente: se indica historial incompleto. Objetos anunciados ausentes o
corruptos fuera de esa frontera producen error.

Se reutilizan validación, lectores y presupuesto de `revision`: árboles hasta
8 MiB acumulados, profundidad 128 y 65.536 entradas visitadas. Cada objeto commit
se limita a 64 KiB y su lectura acumulada se carga al presupuesto del pedido.
Autor/resumen admiten 96/256 bytes con recorte explícito; rutas y OIDs no se cortan.
El archivo shallow normal admite 32 KiB/512 IDs; una redirección configurable no
admitida falla explícitamente. Límites y cancelación son cooperativos, no promesas
universales de RSS o tiempo de I/O.

## Entrega y navegación

`GitRevisionController` añade la consulta al único worker existente: una activa,
un pendiente de metadatos y una entrega, aceptada solo por poll. Conserva una
página cuando se abre una comparación del mismo repositorio/ruta y un commit de
esa página. La página siguiente reemplaza la anterior; no acumula todo el historial.
Cancelar o cambiar raíz invalida página y comparación. No hay segundo documento,
undo, indexador permanente ni aceptación de tareas durante preparación/pintura.

GUI: botón **Historial del archivo** en el detalle, o A dentro del espacio Git.
Usa la ruta seleccionada; si el estado está limpio, admite el documento activo
dentro del proyecto. Se fija al commit histórico seleccionado o al inicio de la
captura aceptada del repositorio. Elegir revisión abre su diff. **Más antiguos**
o N carga la página siguiente; R/F5 relee desde el inicio fijado. **Volver al
historial** regresa sin recargar; **Todos los archivos** vuelve al estado local.
La cabecera se conserva ligada al commit comparado aunque el cursor del listado
se mueva. Errores/cuotas de la comparación se muestran, no como un diff vacío.
En ventanas estrechas se ocultan botones que no caben; las teclas siguen disponibles.

La GUI identifica **Renombrado** y la TUI usa **R** en la fila. El diff
indica los nombres anterior y nuevo. Tras un renombrado, las páginas
anteriores siguen consultando la ruta antigua.

TUI: desde Estado Git, A consulta la ruta seleccionada; sin fila usa únicamente
el archivo activo dentro del proyecto. N pagina, R/F5 relee, Intro compara,
Esc/B vuelve al listado y H regresa al historial general. Se conservan la elección
explícita del codec, los límites visibles y las fuentes históricas inmutables.
No se navega desde el diff histórico al buffer editable como si fuera su revisión.

## Esquema / outline

La GUI muestra una **×** explícita en la cabecera **ESQUEMA**. Ocultar cancela
solo su controlador, retira candidatos/hits y devuelve el foco; una entrega tardía
no lo reactiva. El botón **Esquema** de la barra inferior lo muestra de nuevo.
Se conserva la preferencia de visibilidad en los metadatos existentes, sin cambiar
el formato de recuperación. Ayuda: doble clic o Intro abre; Ctrl permite apertura
lateral. Las consultas léxicas existentes siguen siendo opcionales según lenguaje;
no se añadieron proveedores semánticos, AVM, LSP ni funciones simuladas.

## Validación

Trece casos nuevos dirigidos por suites: cinco del núcleo, dos de entrega, dos de
TUI, dos de GUI Git y dos de cierre/reapertura del Esquema. El flujo GUI recorre
26 revisiones en páginas 24+2, abre la raíz con lado anterior ausente, vuelve,
rechaza hits antiguos y muestra un fallo de blob inyectado. La TUI recorre 28
revisiones 24+4, compara y conserva archivo/buffer sucio/vistas/metadatos Git;
también prueba el archivo limpio activo. Los tests de Esquema verifican navegación
y un solo undo de la edición previa, sin mutación por ocultar/mostrar.

Los primeros intentos tuvieron un fallo transitorio del enlazador con una librería
de caché existente y errores de limpieza de objetos Git readonly en Windows. Se
conservan los logs. La limpieza se corrigió en fixtures propios, sin alterar los
permisos de documentos/repositorios del usuario. El cierre usa enlaces seriales.
Clippy señaló ese ajuste de readonly de los fixtures: se limita y documenta como
operación Windows; Clippy posterior pasó. Sin nuevas dependencias ni cambio al lock.

Resultados finales y hashes en `docs/evidence/g3-path-history-outline-2026-11-06.json`.
No se repitió la suite completa ni pruebas Git ya cerradas, y no hubo escritorio,
portapapeles personal, ConPTY, CI ni push. No se declara una nueva aceptación HWND
o física. IME excluido, Narrador aplazado y segundo monitor sin verificar.
