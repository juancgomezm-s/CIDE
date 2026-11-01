# Puente Progress

El puente evita implementar un compilador ABL propio. JC-IDE prepara una solicitud y una sesión de Progress ejecuta un helper ABL.

## Perfil

Cada proyecto debe declarar explícitamente:

- ejecutable de Progress;
- `.pf` y `.ini` cuando correspondan;
- `PROPATH`;
- `-cpstream` y `-cpinternal`;
- conexiones/aliases requeridos;
- procedimiento de arranque para programas que dependen del ERP.

## Acciones iniciales

- **check**: compilar en un directorio temporal y devolver diagnósticos.
- **compile**: producir r-code en el área de trabajo seleccionada.
- **schema**: consultar tablas, campos e índices de bases conectadas.

La acción genérica **run** se deja fuera del protocolo inicial porque ejecutar un módulo empresarial puede requerir inicialización, parámetros y procesos persistentes específicos.

## Protocolo

La primera versión usa un archivo ASCII de solicitud y una respuesta ASCII con texto codificado en hexadecimal. Así los mensajes pueden conservar bytes de la sesión sin afirmar que son UTF-8.

Registros de respuesta: `DIAG`, `TABLE`, `FIELD`, `INDEX`, `INDEX_FIELD`, `END|OK` y `END|ERROR`.

El parser Rust impone límites de tamaño y rechaza respuestas incompletas o contradictorias.

## Esquema

El helper consulta `_File`, `_Field`, `_Index` y `_Index-Field` mediante `DICTDB`. El catálogo resultante es para navegación/autocompletado; no sustituye una base de esquema cuando el compilador necesita resolver referencias.

## Validación pendiente

Compilar y ejecutar helpers en OpenEdge 10.0B y 11.7, verificar posiciones de error, includes, aliases, conexiones, code pages y rutas reales de Windows.
