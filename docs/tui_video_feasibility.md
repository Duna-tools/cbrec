# Ratón y vídeo dentro de la TUI: viabilidad y límites

Fecha: 2026-11-03. Estado: investigación y sonda local; vídeo no integrado en cbrec.

## Conclusión corregida

Una TUI puede tener ratón y vídeo dentro de su propia superficie. El límite no
es Rust ni Ratatui, sino las capacidades del emulador y cómo se usa cada protocolo.
La pérdida de detalle demostrada en `thumbnail_rendering.md` corresponde al
fallback de bloques; no demuestra que todas las TUI necesiten ese fallback.
No es obligatorio migrar a una GUI para obtener imágenes reales en terminales
compatibles. Tampoco se puede garantizar esa calidad en cualquier terminal de
texto sin soporte gráfico. Multiplataforma no significa compatibilidad visual
idéntica con todos los emuladores de cada sistema.

## Documentación primaria consultada

- [Crossterm, ejemplo oficial de eventos](https://github.com/crossterm-rs/crossterm/blob/master/examples/event-poll-read.rs): captura de ratón, teclado, resize y restauración. Ya usamos este mecanismo.
- [ratatui-image](https://github.com/ratatui/ratatui-image): imágenes Kitty/Sixel/iTerm2 y fallback; la codificación de `StatefulImage` bloquea el hilo que renderiza si no se mueve a un worker. `ThreadProtocol` separa ese trabajo. La biblioteca no aporta por sí sola una reproducción HLS con audio.
- [Protocolo Kitty](https://sw.kovidgoyal.net/kitty/graphics-protocol/): consulta de capacidades, transferencia RGB/PNG, posiciones, borrado y animaciones. Soportar el protocolo básico no garantiza sus extensiones, como Unicode placeholders.
- [mpv, salidas de vídeo](https://mpv.io/manual/master/#video-output-drivers): salidas `kitty`, `sixel` y `tct`. Documenta Kitty probado en Kitty y Konsole. Su ruta de memoria compartida no está implementada en Windows; no puede elegirse como ruta universal.
- [timg](https://github.com/hzeller/timg): otra implementación de vídeo en terminal y fallback; su AppImage mínima no incluye decodificación de vídeo. No se instaló ni se propone como dependencia obligatoria.
- [FFmpeg, protocolos](https://ffmpeg.org/ffmpeg-protocols.html#pipe): permite extraer fotogramas por pipe. Ya forma parte del sistema de grabación.

Context7 se usó para ratatui-image, Crossterm, FFmpeg y mpv; las afirmaciones de
compatibilidad se contrastaron con estas fuentes oficiales y el código instalado.
No se usaron opiniones de foros como prueba técnica.

## Matriz documental; no equivale a QA nativa

| Entorno | Posibilidad documentada | Límite de la evidencia |
|---|---|---|
| Linux, emulador con Kitty | Imágenes reales y actualizaciones por fotograma | Hay que probar las extensiones realmente implementadas |
| Windows Terminal | [Microsoft documenta Sixel desde Preview 1.22](https://devblogs.microsoft.com/commandline/windows-terminal-preview-1-22-release/) | No es una prueba de cbrec en Windows ni de todas sus consolas |
| macOS, iTerm2 | [Imágenes y GIF animado; también documenta Kitty](https://iterm2.com/documentation-images.html) | Un GIF animado no acredita reproducción HLS ni audio |
| WezTerm en Linux/Windows/macOS | [Ratón SGR y protocolos de imagen](https://wezterm.org/features.html) | ratatui-image recomienda su ruta iTerm2; no se probó aquí |
| Terminal sin gráficos | Fotogramas representados como caracteres | Menos detalle; no cumple calidad fotográfica equivalente |

El soporte no se deduce de usar PowerShell, Bash o Zsh: esos son shells, no los
emuladores que reciben y dibujan los gráficos.

## Hallazgo de implementación

Nuestra dependencia ratatui-image 11.1.0 utiliza Unicode placeholders para Kitty
y evita automáticamente Kitty/Sixel en Konsole. La implementación de referencia
[vo_kitty.c de mpv](https://github.com/mpv-player/mpv/blob/master/video/out/vo_kitty.c)
usa transferencias y colocaciones clásicas. Esa diferencia permite investigar
una ruta distinta sin concluir que el terminal carece de todo soporte gráfico.
No se copió el código de mpv ni se modificó el backend de producción.

## Prueba local ejecutada

Sin cuenta, red, cambios de configuración personal ni instalaciones nuevas:

1. FFmpeg instalado generó un clip sintético MPEG-4 de 320×180, tres segundos, fuente de 10 fps.
2. Una sonda aislada consultó Kitty básico: Konsole 26.08.2 respondió `OK`.
3. FFmpeg decodificó 30 fotogramas RGB y la sonda los transmitió por el protocolo clásico, en el mismo terminal. Dos capturas muestran distintos tiempos y fotogramas.
4. Al volver a Inicio, quedaron gráficos anteriores. Borrar por ID y luego probar el borrado de colocaciones visibles tampoco dio una limpieza correcta. Ambos FAIL se conservan; no se incorporan como soluciones.
5. El proceso decodificador terminó con código 0 y se restauraron los atributos del terminal.

La prueba usa Konsole/X11 dentro de Xvfb, sin tocar el escritorio personal. No
abre un reproductor gráfico separado. No mide un FPS sostenido de producción,
CPU, sincronía de audio ni resolución HD. El tiempo total incluye consulta y dos
segundos de espera sobre la pantalla de Inicio; no se divide 30 por ese tiempo
para inferir FPS de reproducción.

Separadamente, una prueba PTY envió un evento SGR mientras la sonda transmitía
los 30 fotogramas: recibió el evento, desactivó captura de ratón y restauró
termios. Se observó ~100 ms de respuesta en esa única muestra. No es un percentil,
no dibuja imágenes en el PTY y no demuestra aún el ratón simultáneo al vídeo en
la aplicación real. La QA existente de cbrec con clic, resize y restauración
volvió a pasar contra `target/release/cbrec`.

| Comprobación | Resultado real |
|---|---|
| Ratón de cbrec, clic en botón y restauración mediante PTY | PASS |
| Consulta Kitty y vídeo visible de la sonda dentro de Konsole | PASS, alcance local descrito |
| Entrada SGR durante transmisión, sonda PTY | PASS, separado de renderizado nativo |
| Limpiar la imagen al volver a Inicio en Konsole | FAIL |
| HLS de una sala real, audio y reproducción/grabación simultáneas | NO EJECUTADO |
| Windows/macOS nativos, SSH/tmux, otros emuladores y DPR | NO EJECUTADO |

## Siguiente incremento mínimo propuesto, todavía no implementado

1. Detectar por consulta las capacidades básicas y las extensiones necesarias; elegir una ruta comprobada, sin confiar solo en variables de entorno.
2. Decodificador FFmpeg independiente del grabador. Canal acotado al último fotograma: el vídeo de vista previa puede descartar fotogramas atrasados; la grabación nunca debe hacerlo por este motivo.
3. Escalado/codificación fuera del hilo de entrada. Un único compositor escribe al terminal; no ejecutar mpv y Ratatui como dos escritores independientes sobre la misma TTY.
4. Control de reproducir/parar mediante ratón y teclado; cancelar una vista no cancela una grabación. Audio y sincronización requieren su propia solución y pruebas.
5. Cerrar el FAIL de limpieza antes de dar la ruta de Konsole por soportada. Repetir cambio de sala, resize, pantalla ampliada, salida y fallos del decodificador en cada pareja sistema/emulador declarada compatible.

Criterios de aceptación: vídeo visible en la propia TUI, entrada atendida durante
carga/decodificación, memoria de cola acotada, cero restos al cambiar de pantalla,
cierre sin procesos huérfanos y conservación de grabaciones/JSON. Sin soporte
gráfico, indicar explícitamente la limitación; no vender bloques como imagen real.

Evidencia local: `/tmp/cbrec-tui-video-research/`: sonda y driver aislados,
`konsole-result.json`, capturas `konsole-phase-0/1/2.png`, intento original en
`first-attempt/`, `pty-driver.json` y `mouse-pty.txt`. La sonda es experimental
POSIX de laboratorio, no un componente multiplataforma de producción.


## Actualización: miniaturas verificadas en la TUI

La corrección posterior de Kitty clásico sí pasó cambio de imagen, zoom,
ampliación, resize y vuelta a Inicio, usando el bucle real de la TUI y una
colocación transparente para forzar el repintado al ocultar la imagen. Detalle,
pruebas y límites en [miniaturas](thumbnail_rendering.md#corrección-aplicada-imágenes-dentro-de-la-tui).
El FAIL anterior pertenece a la sonda original; la integración de vídeo/audio
sigue sin implementarse y no se infiere de la corrección de imágenes fijas.
