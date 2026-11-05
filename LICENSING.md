# Licencias y procedencia

## Código nuevo

El archivo [LICENSE](LICENSE) del repositorio declara MIT para el código nuevo,
con el copyright del propietario. Este texto sustituye la nota antigua que decía
que la elección seguía pendiente; no cambia el contenido de LICENSE ni atribuye
esa licencia a terceros. Los avisos de cada componente se conservan abajo.

Los manifiestos mantienen `publish = false`: el rebranding no publica paquetes,
cambia derechos de terceros ni instala el producto. Los recursos gráficos nuevos
y su procedencia están descritos en [assets/branding](assets/branding/README.md).

## Motor Git embebido

`jc-git` adapta las APIs de gitoxide mediante `gix` 0.90.0, enlazado como
biblioteca Rust. Sus textos MIT y Apache-2.0 y los avisos de las nuevas
dependencias se conservan en `third_party/gitoxide`. El inventario, checksums
y alcance están documentados en [docs/GIT_EMBEDDED_LICENSES.md](docs/GIT_EMBEDDED_LICENSES.md).
El empaquetado incluye estos archivos; las dependencias conservan sus propias
licencias y autorías. Esta integración no cambia la licencia de terceros.

## Port parcial de Lime Text

`crates/jc-core/src/region.rs` adapta operaciones de `Region` y casos de compatibilidad de
[limetext/text](https://github.com/limetext/text), revisión
`a501ee418129fedf3fdca3b0d116231dd14af26c`.

Su aviso y condiciones BSD de dos cláusulas se conservan en
[third_party/limetext-text-LICENSE](third_party/limetext-text-LICENSE) y el código indica
la procedencia. Los cambios usan offsets de bytes sin signo; no se ha trasladado el
buffer Go basado en runas ni su infraestructura de canales.

El documento de bytes, el historial y la integración de este workspace son implementaciones
nuevas. La investigación de PIEW se usa como referencia de arquitectura; sus binarios,
DLL y aplicación gráfica no forman parte del repositorio. Los auxiliares ABL de CIDE
son nuevos y no se presentan como una versión certificada de PIEW.

No se incluyen gramáticas, temas, paquetes de Sublime, componentes GUI, servidores LSP
ni software de Progress. Cada incorporación posterior debe registrar su origen y licencia.
