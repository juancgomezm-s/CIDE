# Licencias del motor Git embebido

## Alcance del inventario

Se comparó el Cargo.lock de la integración gix 0.90.0 con
`git show HEAD:Cargo.lock`, sin modificar Git. Baseline:
`4b08d4861f53d735dbce990058857e2c1e819f62`.
SHA-256 del lock revisado:
`ec9db1afe35e53973b17a8339bf3f25434b86de24db3b7a8a136949236bfc5fb`.

Se identificaron 127 identidades nuevas de paquetes de registro; 52 pertenecen
a la familia gix. Este conjunto incluye entradas condicionales del lock y no
se presenta como un inventario exacto de los binarios finalmente enlazados.

## Archivos conservados

- `third_party/gitoxide/LICENSE-MIT` y `LICENSE-APACHE`: textos originales de gix.
- `third_party/gitoxide/dependencies/<nombre-versión>/`: archivos LICENSE,
  COPYING, NOTICE, COPYRIGHT, AUTHORS y equivalentes encontrados en cada paquete.
- `third_party/gitoxide/DEPENDENCIES.json`: versión, fuente de registro,
  checksum, expresión de licencia original, autores y repositorio de cada paquete,
  más ruta, tamaño y SHA-256 de cada archivo copiado.

Se copiaron 262 archivos de avisos/licencias, 1 317 367 bytes de contenido.
Todos se compararon byte a byte con su origen después de copiarlos. Los 127
archivos `.crate` del cache coincidieron con los checksums de Cargo.lock.
No se copiaron árboles de código fuente, ejemplos ni pruebas de dependencias.

## Licencias múltiples y avisos adicionales

Se conservan las expresiones de licencia de cada paquete, incluidos sus
operadores OR/AND. La mayoría de la familia gitoxide declara MIT o Apache-2.0;
también existen paquetes con Zlib, MIT, Apache-2.0, Unlicense y alternativas
como CC0/MIT-0. `encoding_rs` declara además BSD-3-Clause mediante AND;
`unicode-ident` y `unicode-normalization` incluyen Unicode-3.0 mediante AND.
Los avisos correspondientes se conservaron. `r-efi` declara MIT, Apache-2.0
o LGPL-2.1-or-later como alternativas, no como una obligación acumulativa.
No se reetiquetan todas las transitivas con la licencia de CIDE.

## Omisión del paquete defmt 0.3.101

La primera pasada detectó una ausencia: el adaptador de compatibilidad
`defmt` 0.3.101 declara MIT o Apache-2.0 pero su archivo publicado no incluye
los textos. Se resolvió con las copias del paquete hermano `defmt` 1.2.0,
del mismo repositorio y commit upstream exacto
`719491ca3529abd07205165b9b10e4d988552bc2`. Los metadatos del inventario
registran los paths upstream `defmt-03` y `defmt` y el origen alternativo.
Tras esa conservación no quedan paquetes del conjunto sin archivos de licencia.

## Distribución y cambios posteriores

Incluir `third_party/gitoxide` con el paquete distribuido de CIDE. No se requiere
insertar estos textos en el editor, menús o paneles. Cualquier modificación del
lock exige revisar/regenerar el inventario y conservar nuevos avisos. El script
de recolección local queda en `work/collect_gitoxide_licenses.py`; los archivos
versionables resultantes no dependen del cache para ser distribuidos.

Este bloque solo revisa y conserva archivos de licencia. No prueba compilación,
rendimiento o compatibilidad Git ni altera los módulos de motor o interfaz.
