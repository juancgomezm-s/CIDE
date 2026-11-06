# Avisos de gitoxide y dependencias nuevas

CIDE incorpora `gix` 0.90.0, biblioteca del proyecto gitoxide:
https://github.com/GitoxideLabs/gitoxide.
Autor declarado en su paquete: Sebastian Thiel <sebastian.thiel@icloud.com>.
Licencia declarada: `MIT OR Apache-2.0`. Los archivos `LICENSE-MIT` y
`LICENSE-APACHE` son copias exactas del paquete publicado, sin sustitución
de autores ni cambios de texto.

`DEPENDENCIES.json` registra los paquetes nuevos del Cargo.lock respecto a
`HEAD:Cargo.lock`, sus versiones, fuentes, checksums de archivo `.crate`,
licencia declarada, autores y repositorio. Cada aviso copiado registra tamaño
y SHA-256. Las copias están en `dependencies/<nombre-versión>/` conservando
su ruta relativa original. Este directorio se debe incluir con la distribución.

El inventario es un conjunto conservador de entradas del lock, incluidas
dependencias condicionadas por plataforma o funciones. No afirma que todos
esos paquetes terminen enlazados en cada ejecutable. No incluye el código
fuente de los paquetes ni reemplaza sus licencias por una licencia común.

La versión `defmt` 0.3.101 es un adaptador de compatibilidad cuyo archivo
publicado omite las licencias. Se conservaron los dos textos del paquete
hermano `defmt` 1.2.0, que declara la misma licencia y repositorio y cuyo
`.cargo_vcs_info.json` señala exactamente el mismo commit
`719491ca3529abd07205165b9b10e4d988552bc2`. El origen de esa copia queda
registrado expresamente en el inventario; no se redactó un texto sustituto.

La recolección se reproduce localmente con `work/collect_gitoxide_licenses.py`
mientras estén disponibles los paquetes del cache usado para compilar. El
SHA-256 de Cargo.lock en el inventario permite detectar cuándo hay que revisar
y regenerar estos avisos.

La revisión de 2026-11-06 añadió solo el crate interno std-only jc-fs.
Se compararon todos los registros registry del lock anterior y del actual y los
hashes de cada aviso retenido: permanecen idénticos. DEPENDENCIES.json conserva
el inventario original y añade lock_revalidations; solo actualiza su vínculo
con el lock vigente después de esa comprobación, sin recolocar ni reescribir
los textos de licencia.
