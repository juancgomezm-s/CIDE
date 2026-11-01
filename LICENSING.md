# Licencias y procedencia

## Código nuevo

El propietario todavía no ha elegido una licencia de distribución para el código nuevo
de JC-IDE. Esta base se prepara inicialmente como repositorio privado. No se atribuye
automáticamente una licencia MIT, Apache o BSD al proyecto completo.

Los manifiestos tienen `publish = false` para evitar una publicación accidental en crates.io.
La elección de licencia para distribuir el proyecto es una decisión pendiente del propietario.

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
DLL y aplicación gráfica no forman parte del repositorio. Los auxiliares ABL de JC-IDE
son nuevos y no se presentan como una versión certificada de PIEW.

No se incluyen gramáticas, temas, paquetes de Sublime, componentes GUI, servidores LSP
ni software de Progress. Cada incorporación posterior debe registrar su origen y licencia.
