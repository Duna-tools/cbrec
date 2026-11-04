# Cortes de grabación, bitrate y recuperación

## Diagnóstico del 2026-11-04

El síntoma comunicado es una grabación que termina después de cierto tiempo y,
en ocasiones, vídeo con bloques de colores o aparente reducción de calidad.
No se dispone todavía del tiempo exacto ni del mensaje de la ejecución afectada.

Se hicieron dos capturas públicas de la sala indicada, sin sesión personal y con
`best`, limitadas a tres minutos cada una. Ambas duraron 180,03 segundos y se
decodificaron completas con FFmpeg sin errores. En este intervalo no se reprodujo
el corte ni se verificó visualmente el defecto de colores.

| Captura | Resolución | Cadencia media | Bitrate del contenedor | Tamaño |
|---|---|---|---|---|
| Referencia | 1920×1080 | ≈59,94 fps | 7,008 Mb/s | 157711432 bytes |
| Recuperación habilitada | 1920×1080 | ≈59,94 fps | 7,010 Mb/s | 157750918 bytes |

El segundo ensayo precedió al ajuste de compatibilidad con FFmpeg antiguo. Ese
ajuste y la configuración final se comprobaron con las regresiones locales.
Los datos de prueba, vídeos y logs se conservan fuera de Git en un directorio
privado: `/tmp/cbrec-recording-audit/`. Las URLs multimedia temporales no se
incluyen en este informe ni en los metadatos públicos.

## Qué significa la calidad

`best` selecciona al inicio la variante de mayor altura y después mayor ancho de
banda anunciado. En el catálogo observado había 360p, 540p, 720p y 1080p; la mejor
anunciaba 59,94 fps y 7128000 bits/s. FFmpeg recibe la URL de esa variante y usa
`-c copy`: cbrec no recodifica ni adapta la resolución a la conexión.

El bitrate real no es la resolución. En la captura de referencia varió entre
6,984 y 7,015 Mb/s por intervalos de treinta segundos, manteniendo la variante
1080p. Una variación de bitrate por sí sola no demuestra pérdida de calidad.
La selección actual tampoco cambia automáticamente a una variante nueva que el
servidor publique después de iniciar la grabación.

Los segmentos perdidos o incompletos pueden afectar la decodificación, pero no se
ha demostrado que esa sea la causa de los colores reportados. Tampoco se puede
reconstruir detalle que llegue dañado desde el emisor.

## Defectos reproducidos y corrección

1. **Pérdida de un segmento tras fallos HTTP transitorios.** Un servidor local
   devuelve errores 503 antes de entregar una playlist y un segmento de vídeo.
   Antes de la corrección se guardaban 110 de 120 paquetes de vídeo, aunque el
   proceso podía terminar con éxito. Con recuperación se conservan los 120 y los
   hashes de todos los cuadros decodificados coinciden con la fuente sintética.
2. **Éxito aparente con un segmento incompleto.** Cortar el cuerpo HTTP a mitad
   dejaba 115/120 paquetes y FFmpeg podía devolver éxito. Los reintentos de apertura
   no reparan este caso: el HLS demuxer solo copia una lista limitada de opciones
   HTTP a sus peticiones internas. Ahora se leen también los avisos de FFmpeg y se
   recuerda si hubo paquetes corruptos o segmentos descartados. Aunque el proceso
   devuelva cero, se informa una grabación interrumpida y se conserva el archivo.
   La señal persiste aunque el aviso salga de la cola del log; también se verifica
   al detener o limitar por duración una grabación. No se promete detectar todo
   defecto visual: se usan avisos de integridad emitidos por FFmpeg.
3. **Pérdida del diagnóstico final en grabaciones con muchos mensajes.** Se
   retenían los primeros 64 KiB de stderr y se descartaba el error que llegaba al
   final. Ahora se conserva una cola limitada a 64 KiB y el resumen prioriza las
   últimas ocho líneas, manteniendo los filtros de URLs y cookies.

La entrada HTTP(S) incorpora timeout de lectura de quince segundos, reconexión
en errores de red y estados 429/500/502/503/504, con límite de espera de reconexión
de cinco segundos, y tres reintentos de segmentos HLS. Se conserva el detector
existente que detiene y preserva el archivo si no crece durante 120 segundos.
No se activa `reconnect_at_eof`: el final normal de un segmento no debe reiniciarlo.

`seg_max_retry` existe en FFmpeg 6.1 y 8.1; no en 4.4/5.1. Si el ejecutable
rechaza específicamente esa opción, se repite sin ella **solo si el archivo
está vacío o aún no existe**. En esas versiones antiguas permanece la recuperación
HTTP de la playlist, con la limitación de no disponer de reintentos HLS explícitos. No se reabre
un archivo que ya contiene datos. Esta compatibilidad se verifica con un
ejecutable simulado; no constituye una prueba nativa de todas esas versiones.

Fuentes primarias: [opciones HTTP de FFmpeg](https://ffmpeg.org/ffmpeg-protocols.html#http),
[demuxer HLS](https://ffmpeg.org/ffmpeg-formats.html#hls),
[HTTP en FFmpeg 6.1](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavformat/http.c)
y [HLS en FFmpeg 6.1](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavformat/hls.c).
La lista limitada de opciones copiadas se puede comprobar en
[ffio_copy_url_options](https://github.com/FFmpeg/FFmpeg/blob/n9.0/libavformat/aviobuf.c).

## Verificación y límites

```bash
cargo test --locked --workspace --all-targets --all-features -- --include-ignored
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --check
```

Regresiones principales:

- `transient_hls_segment_failure_keeps_all_video_packets`: FFmpeg real, errores
  HTTP controlados, número de paquetes y comparación de todos los cuadros.
- `partial_hls_segment_is_reported_and_preserved`: corte a mitad del cuerpo HTTP;
  el archivo se conserva y el éxito aparente se convierte en un error de integridad.
- `damaged_input_is_remembered_after_the_warning_leaves_the_log_tail`: aviso
  dividido entre lecturas y memoria del daño aunque el log posterior lo desplace.
- `stderr_reader_drains_input_and_keeps_the_final_failure`: log extenso, cola
  acotada y conservación del error final.
- `legacy_ffmpeg_falls_back_only_before_any_recording_bytes_exist`: compatibilidad
  y protección contra sobrescribir vídeo existente.
- `http_recovery_limits_transient_statuses_and_preserves_normal_eof`: opciones
  de red, estados transitorios y separación de entradas locales.
- Regresiones existentes de cancelación, archivos fragmentados, disco, metadatos,
  elección de calidad y parada de grabaciones desde la TUI.

**Pendiente:** reproducir la ejecución concreta y de duración equivalente al fallo
del usuario. No se ha verificado una emisión de horas, cambios de resolución del
emisor, renovación de URLs caducadas, interrupciones permanentes ni todos los
patrones de corrupción de segmentos. Los reintentos no garantizan recuperar todo
fallo: al agotarse pueden persistir huecos o terminar la entrada. Las pruebas
locales no acreditan el comportamiento nativo de Windows/macOS.


## Corrección final: relevo de segmentos completos

La aplicación comparte un relevo HLS local entre CLI, TUI y watch. Está ligado a
127.0.0.1, protegido por un código aleatorio y vive solo durante esa grabación.
Reutiliza el cliente HTTP y el presupuesto de reintentos existente. Recibe el
recurso completo en memoria y comprueba el cuerpo HTTP antes de entregarlo a
FFmpeg: una respuesta truncada se descarta y se vuelve a descargar. No escribe
segmentos temporales ni crea archivos `.part.mp4`.

Se mantienen la elección de variante, las etiquetas HLS, las claves, los mapas de
inicialización y los rangos. Las URLs firmadas se mantienen en memoria y se
sustituyen por identificadores locales para FFmpeg. Solo se reenvía un rango
validado; las cookies del navegador no se envían al CDN. Los estados de acceso
como 403 se conservan y los fallos finales aportan un diagnóstico sin la URL.

Hay como máximo dos recursos activos por grabación, 32 MiB por recurso y una
caché acotada de direcciones. Al cancelar se cierran el listener y las peticiones.
El límite de lectura HTTP y los reintentos compartidos acotan la descarga. La
entrega local no corta una respuesta por un timeout menor al tiempo de lectura de
FFmpeg. Los recursos que exceden el límite se rechazan; no se entregan truncados.

La prueba que sin relevo perdía 5/120 cuadros ahora conserva los 120. La comparación
incluye el hash de todos los cuadros decodificados. Otra prueba usando H.264 de
una captura real de la sala indicada recuperó un HTTP 503 y un cuerpo cortado a
mitad: **721 cuadros, todos idénticos a la fuente**. Se ejecutó mediante el cliente
de aplicación completo, sin cuentas personales ni imágenes incluidas en Git.

Regresiones añadidas:

- `buffered_hls_relay_recovers_partial_segments_without_losing_frames`.
- `buffered_hls_relay_retries_http_failures_without_changing_frames`.
- Reescritura de playlists, atributos URI, claves, mapas y consultas firmadas.
- Rangos HTTP y conservación de estados de acceso.
- Respuestas truncadas con Content-Length y con Transfer-Encoding chunked.
- Rechazo de recursos excesivos y cancelación durante una descarga incompleta.
- Rechazo de rutas locales no autorizadas y protección de cookies de cuenta.

El caso sin relevo se mantiene como prueba de detección y conservación ante un
aviso de integridad; la ruta de producción utiliza el relevo.

Los datos de prueba H.264 permanecen privados en `/tmp/cbrec-recording-audit/`.
La recuperación no reconstruye contenido que ya venga dañado del emisor ni datos
que el servidor haya dejado de ofrecer. La sala inicial estaba offline al intentar
repetir el ensayo en vivo con esta última ruta. Se prueban otras salas públicas
indicadas por el usuario y se documentan por separado en la evidencia local.
No se creó la rama dev ni se hizo push.


## Verificación final de la reparación

PASS: 288 pruebas de producto y 49 de ejemplos, sin omisiones en Linux;
Clippy sin warnings, formato, diff y compilación release. Cobertura de líneas:
91.96 % (10380/11288).

Dos capturas consecutivas con la ruta reparada confirmaron también el flujo real
de HLS con mapa de inicialización fMP4, en salas públicas indicadas por el usuario:

| Ensayo | Duración | Resolución de todos los cuadros | FPS | Bitrate medio | Cuadros | Errores al decodificar |
|---|---|---|---|---|---|---|
| A | 120,033 s | 1920×1080 | 30 | 5,013 Mb/s | 3601 | 0 |
| B | 120,017 s | 1920×1080 | 60 | 7,015 Mb/s | 7201 | 0 |

Ambos sidecars indican saved. Se comprobaron las dimensiones de cada cuadro,
además de la cabecera del contenedor. Los alias y archivos de esas salas quedan
en la evidencia privada, sin incorporarlos al repositorio.

La pérdida causada por la transferencia truncada reproducida queda corregida
y verificada. Esto no demuestra que fuera la única causa del defecto visual
comunicado originalmente ni reemplaza un ensayo de horas o en Windows/macOS.
No se crearon ramas, commits nuevos ni se publicó nada durante esta reparación.
