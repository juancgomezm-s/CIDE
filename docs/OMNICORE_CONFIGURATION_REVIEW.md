# Revisión de OmniCore dentro de CIDE

La pestaña **Ajustes** del panel OmniCore muestra el modelo seleccionado en CIDE,
su proveedor y el presupuesto de entrada. Revisa también la configuración global:
modelo predeterminado, directorio, cantidad de modelos y proveedores, tipo,
dirección de conexión sin usuario/contraseña/query y método de autenticación.
Las tarjetas se desplazan con la rueda y conservan el tema del editor.

**Cambiar modelo** utiliza el selector existente y afecta a la conversación activa.
**Actualizar** relee los archivos y el catálogo; no reescribe YAML ni credenciales.
El chat, el borrador y la sesión se conservan al cambiar de pestaña. Esc vuelve al
chat. La consulta enviada desde Chat comprueba la conexión real; esta pantalla
no declara una conexión sana solo porque sus archivos sean legibles.

El adaptador cide-omni-turn-v2 añade el modo `config`, que usa el ConfigLoader del
núcleo OmniCore. No crea un Run ni abre el almacén de credenciales. La captura
acepta hasta 128 proveedores, 1024 modelos y 512 KiB por archivo de configuración;
la respuesta de revisión tiene un límite de 128 KiB y 10 segundos. Se ejecuta en
un trabajador, se acepta solo al sondear y no bloquea el pintado. Se excluyen
claves, valores authRef, YAML original y diagnósticos con texto de origen.

Si OmniCore no puede acceder a su configuración o credenciales, el adaptador
presenta un mensaje en español en lugar de terminar con una excepción .NET sin
control. Esto no modifica permisos ni intenta saltarse restricciones del equipo.

## Entregas de desarrollo completas

No presentar una GUI aislada que dependa de un runtime ausente. Después de
exportar su ejecutable y build-manifest.json, ejecutar:

```
python scripts/complete-cide-gui-dev.py --delivery target/<entrega>/debug --runtime <copia-validada-dentro-del-proyecto>
```

El ejecutable debe coincidir con el manifiesto. La admisión reutiliza la del MSI
para bibliotecas, runtime nativo, adaptador y licencias. Copia a una ubicación
nueva, verifica tamaño y SHA-256 de cada archivo, publica el directorio y añade
su inventario al manifiesto. Un runtime existente distinto se rechaza sin
sobrescribirlo. La entrega usada aquí contiene 391 archivos de OmniCore.

## Validación Windows local

- 42 pruebas dirigidas de OmniCore: aprobadas. Dos pruebas con requisitos
  especiales de entorno quedaron omitidas; no se consideran aprobadas.
- Clippy GUI all-targets sin advertencias y compilación debug: aprobados.
- `python scripts/check-omnicore-config.py --runtime <runtime>`: adaptador real
  sobre configuración desechable; revisión válida, omisión de secretos/query,
  YAML inválido y entrada sobredimensionada. Archivos idénticos antes/después.
- Entrega completa e inventario: verificados; preflight de runtime ausente
  rechazado sin modificar el manifiesto.
- Escritorio: se envió desde CIDE una consulta real a ChatGPT gpt-6-luna con el
  archivo Iconos-clases.cls activo. La respuesta visible identificó Demo,
  Nombre/CHARACTER, contador/INTEGER y principal() → destino(). No se usó una
  consulta lanzada fuera de la GUI como sustituto de esta observación.
- La revisión física de la pestaña Ajustes está pendiente: la nueva instancia
  devolvió captura negra y GetCursorPos Acceso denegado tras selección nueva.
  No se declara aprobada ni se envió entrada sobre coordenadas de una captura negra.

La primera entrega de iconos estaba incompleta y se corrigió agregando el
runtime. Su primera consulta también reprodujo una denegación del almacén de
credenciales bajo el proceso restringido de prueba. La consulta útil se verificó
con acceso normal del usuario, sin cambios de permisos. Entrega actual:
`target/omnicore-settings-next/debug`, versión de desarrollo 0.1.13-dev.
No se ha generado ni publicado un nuevo MSI en este bloque; 0.1.12 sigue siendo
la publicación anterior.
