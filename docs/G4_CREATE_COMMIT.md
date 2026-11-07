# Crear un commit local

En la GUI abre el espacio Git y elige **Crear commit** o pulsa
**Ctrl+Mayús+M**. Escribe el mensaje y pulsa **Intro** para revisar. La
pantalla muestra la rama, el autor, el identificador candidato y la lista
de archivos preparados con sus altas, bajas y modificaciones. Desplázate
con la rueda o las flechas. Pulsa **Confirmar commit** o **Ctrl+Intro**
para publicarlo. **Esc** cancela la revisión.

En la TUI abre **Estado Git** y pulsa **C**. Escribe el mensaje, pulsa
**Intro** para revisar y usa **F2** para confirmar. Las flechas y la
rueda recorren los archivos. **Esc** cancela la revisión. En ambas
interfaces puedes comparar previamente el contenido en **Preparados**.

La identidad se toma de `user.name` y `user.email` de la configuración
local del repositorio. Puedes escribirla en `.git/config` en la sección
`[user]`. El campo del editor acepta un mensaje de una línea. El
motor admite un primer commit o un commit normal sobre una rama local.
Las operaciones en curso, la firma obligatoria, HEAD separado y los
índices con conflictos requieren otro flujo.

La revisión construye el árbol en memoria y enumera las rutas que cambian
frente al padre. Al confirmar, la tarea fija HEAD y el índice con sus
bloqueos Git. Revalida el token exacto del índice y compara el nombre de
la rama y el padre. La publicación comprueba el valor anterior de la
rama incluso si estaba empaquetada. Los bytes preparados provienen del
índice y la confirmación protege los documentos modificados en CIDE.

Los reflogs se preparan bajo sus propios bloqueos. El reflog de rama y el
de HEAD se publican después de mover la rama. Un fallo en esa fase se
informa como commit publicado con advertencia. Si falla la publicación de
la referencia y su resultado resulta incierto, la interfaz refresca el
estado y muestra el identificador candidato. Un fallo anterior puede dejar
objetos inalcanzables. Los límites por revisión incluyen 4096 entradas de
índice, 4 MiB de árboles candidatos y 8 MiB de captura por reflog.

Las pruebas dirigidas publicaron un primer commit y uno posterior en
repositorios aislados. También comprobaron un cambio de rama con el mismo
padre, un cambio del índice, un bloqueo de reflog ajeno y contenido en disco
distinto al preparado. Los recorridos de GUI y TUI confirmaron que repetir
Intro durante la revisión conserva la rama y que la acción explícita
publica el identificador que se mostró.

Formato, Clippy y compilación release offline aprobados. La regresión
Git de la GUI pasó 25 comprobaciones con ventana oculta y PATH vacío.
Esa prueba conservó las fuentes y los metadatos del repositorio de prueba.
La escritura se verificó en los recorridos dirigidos.
Entrega en `target/git-create-commit-next/release/`.
