# Compilación local y limpieza — 2026-11-06

Juan autorizó retirar residuos de desarrollo y acelerar los bloques. Se eliminaron
244 directorios de intermedios Cargo: 28.224 archivos, 16.896.172.139 bytes lógicos
(15,74 GiB). No es una medición de espacio físico recuperado en OneDrive.
Se verificaron por SHA-256 los 340 binarios protegidos: ninguno cambió o faltó.
Un directorio examples con binarios referenciados quedó excluido.

Se validaron las rutas absolutas dentro del workspace, el tipo de directorio
Cargo y la ausencia de enlaces/reparse points antes de eliminar. No se tocaron
fuentes, evidencias, .git, toolchain, archivos personales ni depósitos de recuperación.
Los inventarios detallados permanecen en work/cleanup-plan.json y
work/cleanup-result.json; la evidencia resumida versionada acompaña este documento.
La caché debug activa embedded-git-next y la última release blame-next se conservaron.

Para los siguientes bloques usar `python scripts/build-cide-local.py --output
target/<entrega-nueva>/release`. Compila offline/locked en target/build-cache con
dos trabajos por defecto y exporta EXE/PDB/DLL a una ruta nueva comprobando hashes.
No sustituye las entregas anteriores ni lanza aplicaciones. El primer uso construye
la caché; los siguientes reutilizan dependencias. El manifiesto final distingue
una entrega completa; una copia fallida puede dejar un directorio incompleto sin
manifiesto. No usarlo como entrega hasta completar la verificación.

Las pruebas dirigidas reutilizan target/embedded-git-next; se serializan con Clippy
y release. Mantener como máximo estas cachés activas y revisar intermedios antiguos
al cambiar de caché; no borrar entregas ni evidencia para ahorrar espacio.
