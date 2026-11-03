# CIDE: iconos de aplicación

- **TUI y CLI:** `cide-tui.ico` y `cide-tui-*.png`, originales del miembro
  `CIDE_icon_pack/cide/cide_*.png` del paquete aportado por Juan. Este es el diseño
  con iniciales JG y texto CIDE; no el main sin texto del mismo ZIP.
- **GUI:** `cide-gui.ico` y `cide-gui-*.png`, variante naranja/ámbar del mismo diseño,
  generada con la herramienta integrada imagegen según petición explícita. La
  aclaración del usuario fija naranja para GUI y el original verde para TUI.
- `cide-gui-orange-source.png` conserva la salida generada con alfa exterior.
  [Prompt completo](imagegen-prompt.txt) y [procedencia](provenance.json).

Los ICO contienen16/24/32/48/64/128/256. Los PNG verdes se conservaron byte por
byte del paquete; los tamaños naranja se obtienen solo por conversión/resampling
con System.Drawing ya instalado, sin un segundo cambio creativo. Script fuente:
`scripts/prepare-cide-icons.cs`. No se instala un SDK ni dependencia Rust de imagen.

Los builds Windows/MSVC embeben el ICO en el PE de cada binario canónico y alias.
La GUI carga además el grupo101 en su HWND. En TUI es el icono del ejecutable o
acceso directo; el host de terminal decide su icono de pestaña/perfil. La grilla
de texto portable no se sustituye por una imagen ni requiere protocolo gráfico.
Para un perfil de terminal personalizado está disponible el PNG verde.

La raíz del proyecto y nombres internos `jc-*` no se renombraron. El ZIP aportado
se conserva como adjunto local y no se incorpora íntegro al repositorio. Solo se
incluyen los recursos de aplicación elegidos; los otros iconos del paquete no se
declaran ya conectados a funciones que aún no existen.
