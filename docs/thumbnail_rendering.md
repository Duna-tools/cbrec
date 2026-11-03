# Miniaturas: comparación de la página y cbrec

Estado: ruta Kitty clásica implementada y verificada en Konsole 26.08.2/Xvfb.
Fecha: 2026-11-03. Se conservan debajo el diagnóstico y los fallos históricos.

## Evidencia observada

Chromium aislado, viewport 1440×900, DPR 1, página pública
`https://chaturbate.com/tag/gaming/`, sin cuenta ni aceptación de sesión personal:

- Imágenes `RoomCardThumbnail__image`, JPEG natural 480×270, mostradas aproximadamente a 178,83×100,58 píxeles CSS.
- `loading=lazy`, `image-rendering=auto`, `filter=none`, `srcset` vacío en la muestra.
- La página anónima usó `/ribw/{model}.jpg?{timestamp}`; el API público devolvió `/riw/{model}.jpg?{timestamp}`. Se comprobaron ambas variantes para el mismo modelo y timestamp: ambas 480×270. Los archivos difieren; no se infiere la función de la variante ni su equivalencia de contenido.
- El catálogo con fingerprint produjo HTTP 400 `missing_fingerprint` en algunas peticiones y HTTP 200 en otra. No se fabricó un fingerprint ni se usaron cookies personales. La observación de imágenes se hizo sobre el DOM real, no únicamente sobre el catálogo fallido.

En cbrec, `decode_thumbnail` conserva 480×270. El renderizador compartido,
ejecutado directamente con `TestBackend`, produce estas muestras de color:

| Entrada | Panel en celdas | Zoom | Salida efectiva del fallback |
|---|---:|---:|---:|
| JPEG 480×270 | 60×20 | 1 | 60×34 |
| JPEG 480×270 | 60×20 | 2 | 60×34 |
| JPEG 480×270 | 120×40 | 1 | 120×68 |

Se repitió con un patrón sintético, el JPEG del API y el JPEG usado por la web.
No se simuló el algoritmo con otro reductor: se llamó a `PreviewRenderer::render`.
El fallback codifica dos muestras verticales por celda y una horizontal. Por eso
60×34 contiene 2040 muestras frente a 129600 píxeles de la fuente. Es una relación
de resolución, no un porcentaje de calidad perceptual. Zoom recorta la fuente;
no aumenta las muestras disponibles en el panel.

Código upstream del fallback:
[ratatui-image, halfblocks/primitive.rs](https://github.com/ratatui/ratatui-image/blob/master/src/protocol/halfblocks/primitive.rs).

## Prueba controlada contra el navegador

Un único JPEG sintético se pasó al decodificador actual y al navegador. El buffer
real del fallback se reconstruyó a PNG respetando los colores de `▀`, `▄` y
espacio; se amplió por vecino próximo para representar las celdas, sin inventar
detalle. El navegador recibió el mismo archivo fuente, con renderizado normal.

- Diferencia media RGB entre la decodificación de cbrec y Chromium: **0,0429/255**.
- Diferencia media tras pasar por el fallback y ampliar: **91,0550/255**.
- Contraste de las franjas de seis píxeles al mostrar ambas salidas a 179×101:
  desviación estándar **99,44** en la referencia y **17,28** en el fallback.

Estas métricas describen el patrón de prueba; no son un índice universal de
calidad de las fotos. Confirman que el cuello de detalle está después de la
decodificación. Descargar más resolución o cambiar caché no elimina ese límite.
El navegador sí reduce la fuente a píxeles de pantalla, pero no la convierte
previamente en una cuadrícula de caracteres.

## Reproducción

Sin terminal gráfica, cuenta ni red:

```bash
cargo run --locked --offline --example thumbnail_compare -- target/thumbnail-comparison
```

Genera la fuente sintética, el PNG decodificado, las salidas reales del fallback
y `rendering.json`, con comprobaciones de dimensiones, símbolos y límites.
Un segundo argumento permite usar un JPEG/PNG local; sus archivos permanecen en
el directorio de salida elegido.

Comparación adicional, con Playwright disponible en el entorno de QA:

```bash
node scripts/check_thumbnail_browser.cjs target/thumbnail-comparison
```

Usa Chromium headless, un servidor efímero solo en loopback y el patrón generado.
Comprueba la decodificación y la pérdida de detalle, y escribe `comparison.png`
y `browser-comparison.json`. `PLAYWRIGHT_CHROMIUM_EXECUTABLE` permite seleccionar
el Chromium instalado para QA; si se omite usa el de Playwright. No es una nueva
dependencia del producto ni se instala una utilidad al usuario final.

## Contrato del diagnóstico anterior (antes de la corrección)

| Regla | Criterio observable | Situación |
|---|---|---|
| IMG-001 | La fuente conserva sus dimensiones al decodificar | PASS en las tres muestras |
| IMG-002 | La vista de imágenes no descarta detalle convirtiéndolo en caracteres | FAIL en el fallback actual |
| IMG-003 | La calidad gráfica no requiere elegir un terminal ni activar protocolos | FAIL en la solución TUI actual |

La investigación posterior corrigió una conclusión demasiado amplia: estos
resultados del fallback no obligan a abandonar la TUI. Los protocolos gráficos
pueden conservar píxeles reales dentro de terminales compatibles; hace falta
probar capacidades, implementación y limpieza, no solo elegir un nombre de
protocolo. Véase [la investigación de ratón y vídeo](tui_video_feasibility.md).
La independencia total de las capacidades del emulador no puede darse por
cumplida; debe declararse la compatibilidad realmente verificada por entorno.

Antes de cerrar ese cambio deben repetirse las mismas pruebas de imagen,
redimensionado, zoom y limpieza en Linux, Windows y macOS. NO EJECUTADO en
Windows/macOS nativos, pantallas con DPR diferente de 1 ni en la sesión personal.
Sixel y sus restos gráficos constituyen otro problema; no se ha usado cambiar
el terminal como solución del pixelado.

Evidencia local: `/tmp/cbrec-image-comparison/`. Contiene DOM/red observados,
metadatos de las dos variantes JPEG, resultados de la sonda y comparación
sintética. No se añadieron fotos reales ni cookies al repositorio.


## Corrección aplicada: imágenes dentro de la TUI

La selección automática consulta el protocolo Kitty clásico antes del lector de
teclado, con un plazo de 250 ms y una respuesta correspondiente al ID enviado.
Una respuesta ausente o negativa conserva la detección anterior y el fallback.
No se cambia la configuración del terminal ni se inicia otra ventana del producto.
La nueva consulta usa `/dev/tty` en Unix; Windows conserva la ruta existente.

La imagen se transmite como RGB de 24 bits mediante bloques base64 de hasta
4096 bytes, después de dibujar los widgets. Se conserva la proporción, se reduce
solo si el panel es menor y no se amplía artificialmente en zoom 1. La codificación
se reutiliza mientras no cambien fuente, zoom o área. La dependencia base64 ya
estaba en Cargo.lock; ahora también es directa, sin paquetes transitivos nuevos.

El borrado por ID y `Terminal::clear` no bastaron en la prueba inicial de Konsole.
El borrado sigue limitado al ID y colocación de cbrec; cuando la imagen queda
oculta, una colocación RGBA transparente de un píxel fuerza el repintado del
emulador. Esta corrección se comprobó en capturas, también con `NO_COLOR=1`.
Los fallos de Sixel y de las primeras sondas anteriores se conservan como evidencia
histórica; no se presenta Sixel forzado como solución.

| Regla | Evidencia | Resultado |
|---|---|---|
| IMG-001 · conservar los píxeles | Dos patrones RGB 480×270 comparados completamente con la captura de la TUI | PASS: 259200 píxeles por ejecución |
| IMG-002 · evitar la cuadrícula de caracteres | Transferencia Kitty `f=24`, igualdad RGB y prueba del fallback separada | PASS en la ruta gráfica |
| IMG-003 · detección automática | Fixture sin protocolo forzado, Konsole 26.08.2/Xvfb | PASS local; otras plataformas sin verificar |
| IMG-004 · zoom, ampliación y resize | El zoom cambia el bitmap; ampliación y resize conservan ese bitmap completo | PASS |
| IMG-005 · volver a Inicio sin restos | Comparación exacta del cuerpo de Inicio antes y después de las imágenes | PASS con color normal y NO_COLOR |
| NAV-001 · ratón y salida | Clic en la tarjeta Explorar; desactivar captura y restaurar pantalla alternativa al salir | PASS |

Reproducción sin red ni cuentas, con Konsole, Xvfb, FFmpeg, util-linux `script`
y bibliotecas X11/XTest disponibles como herramientas de QA:

```bash
cargo build --locked --example tui_fixture
python scripts/test_tui_graphics.py target/debug/examples/tui_fixture target/tui-graphics-qa
NO_COLOR=1 python scripts/test_tui_graphics.py target/debug/examples/tui_fixture target/tui-graphics-qa-no-color
python scripts/test_tui_pty.py target/release/cbrec
```

La fixture usa el mismo bucle, renderizador y controles del producto; únicamente
sustituye los canales externos por salas y patrones sintéticos. No graba ni abre
navegadores. La sonda `thumbnail_terminal_probe` también ejercita la ruta nativa.
La QA visual inicial se ajustó para excluir el cursor físico del ratón de la
comparación y mapear la ventana correcta en Xvfb; esos fallos del arnés no se
contabilizan como pruebas visuales superadas.

Verificación: 264 pruebas de producto (225 unitarias, 23 de argumentos y 16 de
comportamiento), cero omitidas; Clippy, formato, QA PTY del binario release y
cobertura de líneas 90,85 %, sin exclusiones nuevas. Evidencia local sintética:
`/tmp/cbrec-tui-sharp/qa-complete/`, `qa-complete-no-color/` y `coverage.txt`.

NO EJECUTADO: Windows/macOS nativos, SSH/tmux, otros emuladores, otros DPR,
cuentas autenticadas y vídeo HLS. Una terminal sin soporte gráfico sigue limitada
por las celdas: ninguna TUI puede prometer píxeles independientes de su emulador.
El cambio entrega miniaturas, no reproducción de vídeo ni chat integrado.
