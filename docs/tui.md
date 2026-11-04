# Panel interactivo de cbrec

## Uso

```bash
./target/release/cbrec tui
./target/release/cbrec tui --tag gaming --limit 20
./target/release/cbrec --quality 720p --jobs 2 tui
```

Sin `--tag`, Inicio carga el catálogo público del sitio. El orden lo decide el
servidor y puede incluir promociones; no es una clasificación estricta por
espectadores ni una recomendación personalizada de tu cuenta.

Pulsa `/` o el buscador y escribe `#gaming` (también se conserva `gaming`),
`@usuario` o una URL como `https://chaturbate.com/usuario/`. Enter confirma;
Esc deja de editar y Ctrl+U vacía el campo. Pegar una URL no ejecuta atajos.
Una búsqueda vacía abre el catálogo general. Un perfil se consulta directamente,
sin recorrer el catálogo; si no hay datos de espectadores se indica expresamente.

`--limit` es el tamaño de página (1–50), no un límite total de exploración.
Los botones Anterior/Siguiente y PgUp/PgDn recorren páginas; se muestran el número
de página y el total cuando la API lo entrega. Se conservan las entradas adicionales
promocionadas del servidor. El catálogo cambia en directo, por lo que una sala
puede aparecer de nuevo o cambiar de posición entre peticiones.
Una búsqueda nueva cancela la anterior y descarta sus respuestas tardías.
Cada búsqueda tiene un plazo de ocho segundos; un error conserva la página actual
y `R` repite la solicitud que falló. No se descargan todas las páginas por adelantado.

| Tecla | Acción |
|---|---|
| Enter | Confirmar el buscador mientras se edita; entrar en la sala seleccionada |
| PgUp/PgDn o [ / ] | Página anterior / siguiente |
| p | Cargar populares desde Inicio |
| ↑/↓ o j/k | Seleccionar una sala o grabación |
| o | Abrir la sala en el navegador predeterminado para ver vídeo y comentar |
| l | Abrir el sitio en el navegador para iniciar sesión |
| a | Elegir el navegador abierto y recibir su sesión efímera con Violentmonkey |
| r | Grabar la sala seleccionada en segundo plano |
| s | Detener la grabación seleccionada y conservar lo recibido |
| g | Ver las grabaciones activas; permite detenerlas aunque se cambie de búsqueda |
| c | Cambiar la calidad de la próxima grabación |
| t | Pedir o actualizar la miniatura de la sala seleccionada |
| R | Repetir la última solicitud, incluida la página que falló |
| + / - | Acercar o alejar la miniatura; al acercar se recorta el centro |
| f | Ampliar el panel de la sala dentro de la terminal |
| h / Esc | Inicio / volver a la pantalla anterior |
| q / Ctrl+C | Salir; si hay grabaciones, q confirma su parada antes de salir |

`f` amplía la vista interna. La pantalla completa de la ventana y el tamaño de
fuente se controlan desde el terminal. Las miniaturas son imágenes fijas.
Se conserva su resolución original y se detectan Kitty, Sixel o iTerm2. Cuando
el terminal no ofrece gráficos, se muestran bloques RGB y el panel avisa de la
resolución limitada por celdas. Ampliar texto no recupera detalle de una imagen.

La detección automática consulta primero Kitty clásico, sin placeholders Unicode.
En Konsole 26.08.2 esta ruta conserva los píxeles y limpia la imagen al navegar.
Solo se activa tras recibir una respuesta positiva del terminal; en otros entornos
se conservan la detección de ratatui-image y el modo de compatibilidad:

```bash
CBREC_IMAGE_PROTOCOL=text ./target/release/cbrec tui
```

`CBREC_IMAGE_PROTOCOL` admite `auto` (predeterminado), `text`, `kitty`, `sixel`
e `iterm2`. Los protocolos forzados dependen del terminal. El proyecto upstream
[documenta limitaciones de Konsole/Sixel](https://github.com/ratatui/ratatui-image#compatibility-matrix).
La representación de texto sigue limitada por las celdas; no ofrece la calidad
de una imagen en navegador. No se requiere una ventana gráfica independiente.

Inicio incluye el catálogo y tarjetas pulsables para Populares, Grabaciones y Conectar sesión;
el campo de búsqueda también acepta clic. Hay botones superiores para las
acciones de cada pantalla. Puedes pulsarlos con
el ratón, seleccionar filas con clic y navegar con la rueda. Los atajos de teclado
siguen disponibles. La captura de ratón se desactiva al salir, también si falla
el bucle de eventos. La primera miniatura se pide al recibir resultados y las
siguientes al cambiar la selección; un error aparece dentro del panel de imagen
con la acción de reintento. Las respuestas de otras salas no alteran su carga.

La imagen se descarga por HTTP reutilizando el cliente, sin lanzar FFmpeg y sin
cookies. Se usa `img` del catálogo público, validando host, ruta y modelo; si no
está disponible se conserva la ruta conocida del CDN. La caché en memoria admite
hasta ocho imágenes y 16 MiB decodificados, respeta `Cache-Control` y `Age`, y
nunca supera 15 segundos. Enter reutiliza una imagen todavía fresca; `t` fuerza
una petición. Cambiar de sala cancela la petición anterior. No hay actualización
continua ni imágenes guardadas en disco. La petición tiene límite de 5 segundos,
1 MiB comprimido y 1280×720 píxeles. FFmpeg sigue siendo necesario para grabar.

## Sesión, chat y conservación

El panel y el navegador tienen sesiones independientes. `o` utiliza el perfil
que abra el navegador predeterminado; allí puedes comentar si tienes sesión y
permisos de la sala. cbrec no envía comentarios ni usa endpoints históricos de
mensajería sin verificar. Tampoco envía cookies a ese proceso ni a las miniaturas.

Para conectar la sesión del grabador:

1. Mantén abierto el navegador y la pestaña de Chaturbate con tu sesión ya iniciada.
2. Instala Violentmonkey y [el userscript de cbrec](../scripts/cbrec-violentmonkey.user.js), según el README.
3. Pulsa `a` y elige Firefox, Chrome, Chromium, Edge, Brave u otro navegador con flechas y Enter, número o clic.
4. Ejecuta «Enviar sesión a cbrec» en esa misma pestaña y pega la dirección local que muestra la TUI.

Elegir el navegador guía la transferencia: cbrec no detecta ventanas, lee perfiles
ni inicia sesión automáticamente. Esc o el botón Cancelar cierran el receptor
pendiente y conservan la sesión anterior. La dirección es de un solo uso y caduca
en 120 segundos. La cookie se mantiene
en memoria; «Sesión recibida» no acredita identidad ni acceso hasta consultar
una sala. Sigue disponible `cbrec --browser-session tui`.

Cada grabación reutiliza la reserva de ruta, descarga, validación y sidecar JSON
existentes. No se crean `.part.mp4`. `s` y la salida del panel usan cancelación
cooperativa y esperan la finalización. Los archivos no vacíos se conservan incluso
cuando la descarga falla. Las grabaciones ya iniciadas conservan la sesión y
calidad con las que se lanzaron; cambiar esos valores afecta a las siguientes.

## QA y límites

| Contrato | Verificación |
|---|---|
| Inicio con catálogo; compatible con `--tag` | `tui_can_start_at_home_without_a_tag`, `parse_tui_command`, pruebas de páginas del cliente |
| Teclado, edición y pantallas independientes | Pruebas del estado y rendering en `presentation/tui.rs` |
| Sin grabar una sala oculta desde Inicio | `recordings_remain_selectable_after_changing_search_and_home_cannot_record_a_hidden_room` |
| Zoom visible en una terminal pequeña | `preview_zoom_keeps_source_resolution` |
| Reserva, parada y salida sin perder vídeo/JSON | `tui_worker_cancels_recording_without_losing_bytes_or_sidecar` |
| Búsqueda fallida, recuperación y sesión por eventos | `tui_worker_keeps_search_errors_and_session_transfer_inside_events` |
| Teclado, clic real, resize y restauración de termios/captura de ratón | `tui_home_keyboard_resize_and_terminal_restoration_in_pty`, script POSIX `scripts/test_tui_pty.py` |
| Resolución original y rechazo de imágenes inválidas/grandes | `decoder_preserves_source_resolution_and_rejects_invalid_images` |
| Caché, caducidad, cambio de URL y memoria acotada | `cache_reuses_pixels_expires_and_evicts_without_growing` |
| Host/ruta, tipo, tamaño y frescura del CDN | Pruebas de `infrastructure/external/thumbnail.rs` |
| Cancelar una petición bloqueada al cambiar de sala | `selecting_another_thumbnail_cancels_a_stalled_request` |
| Gráficos nativos y fallback RGB | Pruebas de `presentation/thumbnail.rs` |

La QA descubrió que `Child::wait` cerraba stdin antes de que se pudiera enviar `q`.
Se conserva ahora el handle antes de esperar al proceso y se entrega al cierre
compartido. La regresión comprobó tanto `s` como la salida con una grabación activa.

La prueba PTY también reprodujo entrada atascada al coincidir una tecla y resize
con el backend Mio de Crossterm. Se habilita su alternativa oficial `use-dev-tty`
solo en Unix, sin cambiar la versión del crate. Añade únicamente la dependencia
transitiva `filedescriptor`. La hipótesis es pérdida de notificación de readiness
entre los dos eventos; no se afirma haber corregido el código upstream. El mismo
escenario pasó tres veces con el backend alternativo. Selección de backend:
[código de Crossterm](https://github.com/crossterm-rs/crossterm/blob/master/src/event/source/unix.rs).

Comprobación histórica del primer incremento, 2026-11-03, sin cookies: descubrimiento de una sala por tag
y decodificación de su miniatura con FFmpeg, 6912 bytes RGB esperados/obtenidos.
No prueba estabilidad futura del endpoint, permisos privados ni chat autenticado.
La cuenta real y las plataformas Windows/macOS siguen pendientes de prueba nativa.

## Referencia visual: MovieBox-Tui

Referencia indicada por el usuario: [MovieBox-Tui](https://github.com/mesamirh/MovieBox-Tui).
Se revisó su demostración, el inicio y la pantalla de resultados: marca centrada,
buscador protagonista, accesos breves y resultados en filas con portadas.
Su documentación confirma que la reproducción se delega a un reproductor externo,
como mpv o VLC; no muestra vídeo incrustado en la terminal.

Primer ajuste aplicado: Inicio centrado con marca, buscador, accesos de grabación
y sesión, y calidad/concurrencia visibles. Se mantiene el control de sesión,
la navegación por teclado y el comportamiento del grabador.

El panel muestra gráficos nativos para la sala seleccionada, con detección y
fallback a texto. Quedan pendientes las portadas junto a cada fila y la
integración opcional de un reproductor externo. El chat sigue en el navegador.

## Medición de miniaturas · 2026-11-03

La API pública devolvió `img` con JPEG 480×270 y el CDN indicó `max-age=15`.
La implementación anterior reducía cada imagen a 64×36 antes de dibujarla:
la fuente tiene 56,25 veces los píxeles de esa reducción.

Benchmark local reproducible, JPEG sintético 480×270 y siete pares de muestras,
binario release: mediana FFmpeg 47,97 ms; decodificador nuevo 0,515 ms. Incluye
arranque del proceso antiguo; excluye descarga, latencia de red y renderizado.
No es una promesa de que la navegación completa sea 93 veces más rápida.

```bash
cargo run --release --locked --example thumbnail_benchmark
```

La prueba gráfica inicial usó Konsole 26.08.2 y Xvfb, con una imagen sintética y
sin cuenta ni cambios de configuración personales. Solo observó una imagen
estática mediante Sixel. La QA posterior de cambios de imagen, zoom, tamaño y
vuelta a Inicio reprodujo restos gráficos; intentar `Terminal::clear` también
falló y esa mitigación se descartó. El ejemplo manual `thumbnail_terminal_probe`
ejercita ahora esas transiciones durante diez segundos.
La página principal respondió, pero su petición de catálogo con fingerprint dio
HTTP 400 en Chromium anónimo; el endpoint público por tag sí respondió. No se
afirma haber validado el catálogo autenticado ni toda la API de la web.

En tres ensayos iniciales de red FFmpeg falló sin producir píxeles; un ensayo
posterior sí funcionó. Se conservan esos fallos y no se usan para calcular la
mejora de velocidad. Evidencia local: `/tmp/cbrec-thumbnail-audit/`.


## Calidad gráfica verificada

Kitty clásico conserva los píxeles de la miniatura sin convertirlos en texto.
La prueba automática compara dos imágenes completas de 480×270 con capturas de
la TUI real en Konsole 26.08.2/Xvfb; también comprueba clic, zoom, ampliación,
resize, regreso a Inicio y salida, con y sin NO_COLOR.
La caché evita recodificar y reenviar una imagen estática en cada frame.
Véanse [el informe y los comandos de QA](thumbnail_rendering.md#corrección-aplicada-imágenes-dentro-de-la-tui).
No se ha verificado esta ruta en Windows/macOS nativos ni en todos los emuladores.


## Investigación y aceptación del catálogo

Véase [el análisis de conexión entre web, API y TUI](tui_discovery.md): reglas,
fuentes públicas inspeccionadas, pruebas de paginación, URLs y asistente de sesión.
