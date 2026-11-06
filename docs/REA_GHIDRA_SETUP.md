# REA con decompilador nativo configurado

5 de noviembre de 2026. Juan autorizó preparar un motor para continuar la
ingeniería inversa de Sublime Merge. Esta instalación es una herramienta de
desarrollo, no una dependencia ni un componente distribuido de CIDE.

## Instalación realizada

- REA existente: 6.8.0; Node existente de su runtime: 24.22.2.
- Ghidra: 12.1.4, distribución oficial extraída en
  `C:/Users/juanc/Documents/Codex/rea-runtime/ghidra_12.1.4_PUBLIC`.
- JDK Temurin: 21.0.13+9 x64, extraído en
  `C:/Users/juanc/Documents/Codex/rea-runtime/jdk-21.0.13+9`.
- Java no se añadió al PATH ni al entorno global. No se instalaron servicios,
  extensiones de Ghidra ni otro paquete npm. Se conservaron las licencias de
  ambas distribuciones y sus archivos descargados.

La [distribución oficial de Ghidra](https://github.com/NationalSecurityAgency/ghidra/releases/tag/Ghidra_12.1.4_build)
publica el SHA-256 `ddac49f903da9d5bac833e5cc79395098b9c33cfd3279be5f31bd00387d2d4db`;
se verificó antes de extraer. Para el JDK se usó la API oficial de Adoptium y
se verificó `a4dbaa05fca4bd89d84584e58c459bd732efebb1722af80ab13c6516832004f9`.
URLs, tamaños y hashes exactos: `work/rea-ghidra-download-plan.json`.
Resultado y directorios extraídos: `work/rea-ghidra-installed.json`.

## Configuración persistente

La sección existente `mcp_servers.rea` de `C:/Users/juanc/.codex/config.toml`
recibió únicamente una tabla de entorno con `GHIDRA_INSTALL_DIR`, `JAVA_HOME`
y `GHIDRA_HEADLESS_MAXMEM=1G`. Se validó TOML y se contrastó el objeto completo
para comprobar que el resto de la configuración quedó igual.

Respaldo previo:
`C:/Users/juanc/.codex/config.toml.rea-ghidra-20261105-223921.bak`.
No se copiaron configuraciones personales completas al repositorio.
La configuración de otros clientes y proveedores no se modificó.

Una importación posterior para trazas alcanzó el plazo predeterminado de
330.000 ms; no se confirmó falta de memoria. Se preservó ese error y se comprobó
`close_binary` exitoso. Siguiendo su remedio, se añadieron
`REA_GHIDRA_STARTUP_TIMEOUT_MS=900000` y `tool_timeout_sec=1100` solo a REA,
con otro respaldo: `config.toml.rea-ghidra-timeout-20261105-225149.bak`.
No se deshabilitaron analizadores ni se alteró el límite de heap.

El servidor MCP ya abierto conserva el entorno anterior: toma los ajustes al
reiniciarse Codex. Durante esta sesión se usó el mismo REA existente con entorno
explícito por consola y un cliente MCP de investigación separado. No se necesita
reiniciar para continuar el análisis actual. El patrón de entorno por servidor
está descrito en la [documentación oficial de MCP](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

## Validación observada

`work/rea-ghidra-doctor.json` informa `healthy=true` para el alcance explícito
Ghidra: versión, launcher headless, decompilador nativo PE x64, JDK completo de
64 bits y controles Windows de Job Object, DACL privada y handles sin reparse.
El diagnóstico conjunto posterior `work/rea-ghidra-codex-doctor.json` pasó sus
11 comprobaciones seleccionadas, incluida la configuración de Codex.
Los diagnósticos informativos de proveedores no configurados no se presentan
como aceptación de todos los motores de REA.

Una búsqueda real de `side_bar_layout` en `sublime_merge.exe` pasó mediante
Ghidra 12.1.4. La evidencia es
`ev_678f532fd3ab1fedaf2b01d6436bd77dc1b1f1cc86311526bc3262504266db53`,
dirección `0x1406f3257`, SHA-256 del ejecutable
`f02ffd91ef0fdb7983c27b237888c433bd6ba9f420e57752a41755eeda08387e`.
La operación se entrega después de completar el autoanálisis; no ejecutó la
aplicación. Resultado: `work/rea-sublime-merge-native-layout-search.json`.

Se verificaron en el JVM propio los argumentos efectivos `-Xmx1G`,
`-XX:ParallelGCThreads=2` y `-XX:CICompilerCount=2`.
Eso limita heap y esos pools, no todo el RSS ni todos los hilos del análisis.
No se alteraron afinidad, prioridades ni otros procesos.

Las tres trazas y tres decompilaciones posteriores terminaron y se conservan en
`work/rea-merge-native-long-full-evidence.json`: seis registros REA.
La sesión cerró correctamente; se verificó que el ejecutable original quedó intacto.
[Resumen portable](evidence/rea-ghidra-setup.json). El resultado no es el código fuente original: nombres,
tipos, llamadas indirectas y comportamiento en ejecución requieren distinguir
observación, inferencia y desconocidos. El PDB es una referencia del binario,
no una prueba de que se hayan cargado símbolos originales.

## Reutilización

Tras reiniciar el cliente, REA puede seleccionar Ghidra automáticamente o con
`provider_id=ghidra`. En el workspace, `work/rea_ghidra_run.py` permite invocar
la instalación existente con el entorno verificado sin cambiar el sistema.
El script de investigación `work/rea_merge_native_study.mjs` mantiene una sola
importación para varias trazas y decompilaciones, exporta evidencia y cierra
su base efímera al terminar. Usa el cliente MCP ya incluido en REA.

Si una consulta nueva excede el tiempo o encuentra un formato no admitido,
debe tratarse ese fallo concreto. No reinstalar el motor que ya pasó su prueba
ni afirmar decompilación completa por el mero diagnóstico de disponibilidad.
