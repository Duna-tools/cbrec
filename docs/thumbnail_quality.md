# Nitidez de miniaturas en paneles grandes

La ruta de ratatui-image usaba `Resize::Scale` incluso sin zoom: una fuente de
480×260 terminaba codificada como 1200×660 en el panel de prueba. La ampliación
interpolaba píxeles y podía hacer que la imagen se viera borrosa.

Ahora los protocolos gráficos de esa ruta usan `Resize::Fit` sin zoom. Si la
imagen cabe, conserva sus píxeles; si no cabe, se reduce proporcionalmente con
Lanczos3. El zoom explícito con `+` conserva la ampliación del recorte.
El modo de bloques de texto aprovecha las celdas disponibles para no perder
representación de detalle. La ruta Kitty clásica ya limitaba el tamaño original.

La regresión decodifica el PNG emitido por iTerm2 y compara dimensiones y todos
los píxeles originales; también verifica reducción, zoom y uso de celdas en texto.
La prueba falló con Scale y pasó tras la corrección.

```bash
cargo test --locked --lib presentation::thumbnail::tests
```

Contrato de la dependencia: [Resize de ratatui-image](https://docs.rs/ratatui-image/11.1.0/ratatui_image/enum.Resize.html).
La cuadrícula del terminal puede añadir relleno de borde de menos de una celda.
Esta corrección no recupera detalle que no exista en la imagen del CDN, ni elimina
la resolución limitada del modo texto. No se verificó iTerm2/Sixel en hardware
nativo de Windows/macOS: la prueba del protocolo utiliza TestBackend y PNG real.
