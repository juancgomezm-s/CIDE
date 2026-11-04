# PIEW 5.80 como referencia para los exploradores de CIDE

Análisis estático de la copia local de `piew.zip`. El ZIP incluye material adicional y puede diferir del publicado en el sitio. REA identificó y extrajo el paquete, clasificó el ejecutable como CIL de .NET Framework 4.8 e inspeccionó sus miembros. ILSpy local recuperó los cuerpos en C# aproximado. Los nombres internos y varias cadenas están ofuscados. El análisis no ejecutó PIEW ni se conectó a OpenEdge.

## Code Explorer

El manual enumera procedimientos, funciones, triggers, tablas temporales, work-tables, búferes, métodos y propiedades. La rama de métodos y propiedades depende de la extensión `.cls`. El ejecutable usa un trabajador de fondo, ramas de análisis de código estructurado y reconocimiento por líneas. La segunda aplica expresiones regulares y verifica candidatos contra el editor. Agrupa por clase de símbolo, restaura la expansión y asocia cada entrada a un marcador de línea para saltar al código.

Anclas en `work/piew-reference/Piew.decompiled.cs`: análisis desde 15406, agrupación 15718, trabajador y expansión 15803, salto 15931. Este flujo establece navegación léxica. La resolución de usos a declaraciones para renombrar por ámbito requiere un proveedor adicional.

## Database Browser

1. En una sesión ABL con las bases ya conectadas, `DB_Dump/Dump.p` recorre `NUM-DBS`, obtiene cada alias lógico con `LDBNAME`, reasigna `DICTDB` y ejecuta `Dump_Tables.p`.
2. El segundo script une los metadatos `_File` y `_Field` y filtra tablas ocultas y nombres que comienzan con guion bajo. Escribe un CSV por base, ordenado por tabla y campo, con cinco columnas separadas por punto y coma: tabla, campo, tipo, formato y etiqueta. El script elimina saltos de línea de la etiqueta y no escapa los separadores.
3. El editor carga bajo demanda esos CSV o definiciones Progress `.df`. Guarda los esquemas en diccionarios en memoria con claves en minúsculas y construye un árbol base → tabla → campo. El manual del exportador indica que vuelve a cargar los archivos al reiniciar. El ejecutable muestra tipo, formato y etiqueta y usa el catálogo para el navegador. El manual documenta también autocompletado e inserción desde el árbol.

Anclas de la descompilación: carga y normalización 2531, parser CSV 2585, parser DF 2636, árbol 1727. El exportador ABL se distribuye como código fuente dentro del ZIP.

## Aplicación en CIDE

- **Esquema de código.** Evolucionar el panel bajo el Explorador a categorías expandibles con rangos tipados. Reutilizar el lexer ABL y las tareas versionadas existentes. Guardar expansión por categoría y aceptar saltos únicamente para la revisión visible. Agregar ámbito de declaración como información del proveedor de código, antes del renombrado.
- **Explorador de bases.** Implementar un catálogo local en Rust, importable desde un archivo y consultable sin AVM. Empezar con el protocolo tipado existente para tablas, campos e índices. Ofrecer importación del CSV de PIEW como compatibilidad y admitir `.df` tras validar una gramática y archivos de prueba representativos. Separar perfil y alias lógico para evitar confundir dos bases con el mismo nombre.
- **Actualizar esquema.** Ejecutar el puente ABL existente bajo demanda con el perfil elegido. `progress/abl/jc-bridge.p` y `jc-schema.p` ya contienen el recorrido de bases y metadatos con `NO-LOCK`. Rust debe validar que la captura terminó y que sus referencias internas son coherentes, guardar una generación completa y publicar el catálogo de forma atómica. Si falla la actualización, conservar la última captura válida. La navegación y el autocompletado consumen exclusivamente el catálogo local.
- **Validación.** Probar importación, metadatos duplicados o truncados, codificaciones, recuperación ante interrupción y navegación sin conexión. Verificar por separado el puente con una base de prueba en cada versión de OpenEdge admitida. El puente todavía requiere esa validación.

## Identidades y evidencia

- ZIP SHA-256 `172a5e543397f324ed01c02a543bc6c275232e62cfd17c4aac77bdd2b4568283`. Extracción REA `ev_ba92401420b7916a3709b36886c108ed020b795f8b2fa6bd55a7a520c8888504`.
- Ejecutable `5.80.7975.20511`, SHA-256 `8a9cbdfde68d498936f8bead6334bc1b2a39244ce9abd170d4242d49046aa029`. Metadatos REA `ev_ebc2e18028dea6440d46af38711a1c21c3e254e4492adc379c0d9c2b4fd851ae`. Miembros CIL `ev_528a3db1fd1b8562e90e99671afd4fd59b52556ef6f33036ff247e7aef9e2c4c`.
- [Página pública del autor](https://progress-tools.x10.mx/piew.html).

## Procedure Explorer de PUtils

La [ficha del autor](https://progress-tools.x10.mx/putils.html) documenta en
Procedure Explorer 1.61 navegación de dependencias en ambas direcciones:
programas llamados desde el archivo, archivos que hacen referencia al programa
e indagación recursiva hacia los posibles puntos de entrada. También documenta
extracción de inclusiones desde el segmento de depuración de archivos `.r`
y compatibilidad con ciertos includes de MFG. El servidor devolvió 502 al
consultar la página directamente y no se encontró el paquete local. Estos
hallazgos proceden de la ficha pública indexada, sin inspección del binario.

Para CIDE, ampliar el panel lateral con **Llama a**, **Llamado por** e
**Inclusiones**. El primer bloque debe reutilizar el proveedor ABL de
referencias y recorrer los fuentes del proyecto bajo demanda. Cada relación
conserva el archivo y rango que la originó, el ámbito de origen cuando pueda
determinarse y el estado de resolución. Respetar el orden de PROPATH cuando
esté configurado, mostrar destinos ambiguos y separar llamadas dinámicas de
enlaces resueltos. La navegación inversa debe detectar ciclos, acotar
profundidad y resultados y mostrar posibles raíces estáticas. La lectura de
metadatos de `.r` y las convenciones de MFG son un bloque separado, sujeto a
muestras representativas. La búsqueda de dependencias no autoriza
renombrados hasta comprobar la identidad de cada referencia.
