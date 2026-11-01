# M1 — Archivos, encoding y posiciones

**Estado: cerrado.**

M1 agrega dos capas sin cambiar la regla fundamental de JC-IDE: el documento canónico sigue siendo una secuencia de bytes.

## jc-text

Codificaciones iniciales:

- UTF-8, con BOM preservado en los bytes y omitido solamente de la vista textual;
- Windows-1252;
- ISO-8859-1;
- IBM850.

La detección automática es deliberadamente limitada. BOM UTF-8 y UTF-8 válido son verificables. ASCII es compatible con varias code pages y se etiqueta como ambiguo. Un stream no UTF-8 queda como `UndeterminedLegacy` hasta que proyecto o usuario elijan encoding.

Una decodificación no puede modificar el documento. Bytes indefinidos o secuencias UTF-8 inválidas producen U+FFFD **solo en la vista**, junto con el rango original que causó la incidencia. Los bytes siguen intactos.

`encode()` es estricto para inserciones: nunca sustituye silenciosamente por `?`.

## Line endings y coordenadas

Se inspeccionan CRLF, LF, CR, mezclas y newline final sin normalizarlos. `TextIndex` convierte entre:

- offset del byte fuente;
- línea lógica, base cero;
- columna en escalares Unicode, base cero.

Una posición dentro de la mitad de un CRLF se rechaza. Graphemes, columnas visuales, UTF-16 de LSP y coordenadas del compilador siguen siendo dominios distintos y se convertirán en sus respectivos adaptadores.

## jc-files

`FileSession` une ruta, documento, encoding seleccionado y versión conocida del disco.

Save normal:

1. comprueba que el contenido del disco coincide con la versión conocida;
2. escribe a un archivo temporal del mismo directorio;
3. preserva permisos cuando sustituye un archivo;
4. flush + sync del temporal;
5. publica de forma atómica;
6. sincroniza el directorio donde la plataforma lo permite;
7. solo entonces marca la revisión como guardada.

En Unix el reemplazo existente usa `rename`. En Windows usa `ReplaceFileW`. La creación de un archivo nuevo usa hard-link no-clobber del temporal al nombre final, por lo que un archivo que aparezca durante la operación no es sustituido.

Si el writer falla antes del commit, el temporal se elimina y el original permanece intacto. Si otro proceso modifica o elimina el archivo, Save devuelve conflicto; `force_save` es una acción deliberada distinta.

La detección de cambio usa una huella de contenido de 128 bits no criptográfica más longitud. Su objetivo es detectar conflictos editoriales, no demostrar integridad frente a un adversario.

## Historial y memoria

`Document` limita undo/redo a 1024 pasos por defecto. El límite es configurable; cero desactiva historial. El buffer de inserciones sigue siendo append-only durante la edición para conservar la simplicidad de la piece table, y `compact_storage()` permite consolidar el contenido actual a costa de descartar undo/redo.

La política automática de cuándo compactar pertenece al frontend/session manager y se decidirá con mediciones de archivos reales.

## Lo que M1 no afirma

- No afirma que todos los proyectos ABL del usuario empleen una misma code page.
- No convierte automáticamente legacy a UTF-8.
- No implementa grapheme shaping, IME ni columnas visuales; eso corresponde a TUI/GUI.
- No sustituye la validación de `-cpstream` / `-cpinternal` de cada perfil Progress en M3.
- No declara todavía rendimiento de archivos enormes; los presupuestos se medirán con el frontend.

La consecuencia es intencional: M2A puede construir la TUI sobre un ciclo de archivo que ya tiene semántica definida, sin poner la codificación o el guardado dentro de widgets.
