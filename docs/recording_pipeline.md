# Contrato de grabación y rendimiento

Alcance: cbrec, grabación directa y `watch`. No se modifican el sistema operativo,
los streams remotos ni las grabaciones existentes.

## Reglas observables

| Regla | Contrato | Verificación |
|---|---|---|
| BR-001 · P0 | Reservar el `.mp4` con creación exclusiva; nunca crear un vídeo `.part.mp4` nuevo ni reutilizar un nombre ocupado | Reserva directa, concurrencia, colisiones con metadatos y parciales antiguos |
| BR-002 · P0 | Un fallo conserva los bytes disponibles; se informa el fallo y se registra `classification: interrupted` | Fallo de descarga, reintento, cancelación y fallo de metadatos |
| BR-003 · P0 | La cabecera MP4 sola no es una grabación correcta | Validación de `moov`, `mdat` con datos y límites de cajas |
| BR-004 · P1 | Los fragmentos completados deben poder leerse sin el cierre final | FFmpeg/ffprobe, truncamiento y terminación forzada |
| BR-005 · P1 | Una sala disponible empieza a grabarse sin esperar otra consulta lenta | Prueba de `watch` con una consulta bloqueada hasta que comienza la descarga |

```gherkin
# language: es
Escenario: Conservar una grabación interrumpida
  Dado que una grabación ya recibió contenido multimedia
  Cuando la descarga falla
  Entonces el archivo mp4 conserva los bytes recibidos
  Y sus metadatos indican interrupted
  Y el fallo se comunica sin anunciar una grabación correcta
```

Este escenario se verifica mediante las pruebas Rust existentes; no se añade un
runner Gherkin. Blancos de mutación propuestos, NO EJECUTADOS como campaña:
eliminar `create_new`, aceptar solo `moov`, omitir `flush_packets` o volver a
esperar todas las consultas antes de iniciar la descarga.

El vídeo existe durante la descarga. El JSON confirma su resultado; si falta,
el archivo no debe considerarse terminado. Una interrupción posterior a recibir
datos no se presenta como éxito: grabación directa devuelve error y `watch`
aplica la pausa de recuperación. Los JSON temporales mantienen su escritura
atómica; no son vídeos `.part.mp4`.

## Piezas y contratos

- CLI/controlador: valida parámetros, configura sesión, calidad, duración y cancelación.
- Cliente HTTP: resuelve estado y variante con límites y reintentos; no envía cookies a FFmpeg.
- FFmpeg: copia los paquetes, escribe fragmentos de hasta cinco segundos o delimitados por keyframes y vacía buffers; supervisa disco, estancamiento y cierre.
- Grabación: reserva el nombre, conserva datos, valida estructura y clasifica el resultado; mueve a `small/` solo grabaciones correctas bajo el umbral.
- Grabación directa y `watch`: observan el mismo archivo `.mp4`, escriben los mismos metadatos y comunican los fallos.
- Configuración/lista persistente y sesión de navegador conservan sus contratos. `check`, `list`, `discover` y TUI no escriben vídeos.

## Medición sintética local

2026-11-01, Linux, FFmpeg n9.0.2. Tres repeticiones alternadas por modo.
Fuente H.264 negra de 64×64 a 60 fps, dos segundos repetidos hasta representar
cuatro horas; copia sin recodificación. No son cuatro horas de ejecución real.
El pico RSS se midió con `wait4`; el tiempo con reloj monotónico.

| Métrica | MP4 anterior | MP4 fragmentado con vaciado |
|---|---:|---:|
| RSS mediano | 110508 KiB | 54324 KiB |
| Tiempo mediano | 3,070 s | 3,104 s |
| Tamaño | 16697836 bytes | 18526436 bytes |

La reducción de RSS es aproximadamente 51 %. No se atribuye mejora de velocidad:
la diferencia temporal está dentro de la variación observada. El tamaño crece
aproximadamente 11 % en esta fuente de bitrate muy bajo; no se extrapola a vídeo real.
El hash de los paquetes comprimidos coincide en ambos modos:
`06e7b4863b362d3e1cf05f0e755bb45ab74dc28798a7cc497698e46e4ff9d156`.

Comando base: `ffmpeg -v error -stream_loop -1 -i source.mp4 -t 14400 -c copy -y output.mp4`.
El modo nuevo añade antes de la salida:
`-movflags +frag_keyframe+empty_moov+default_base_moof -frag_duration 5000000 -flush_packets 1`.
Comparación del contenido: `ffmpeg -v error -i output.mp4 -map 0:v -c copy -f hash -hash sha256 -`.

La terminación forzada a los 2,5 segundos mostró por qué el vaciado es necesario:
sin él quedaron 28 bytes ilegibles; con él se conservaron 120 paquetes legibles.
No garantiza recuperar el fragmento en curso, reparar vídeos antiguos ni cubrir
fallos físicos de disco. La compatibilidad con editores distintos de FFmpeg está pendiente.

Documentación de referencia: [fragmentación MOV/MP4 de FFmpeg](https://ffmpeg.org/ffmpeg-all.html#Fragmentation).

## Verificación del cambio

- PASS: 197 pruebas con `cargo test --locked --offline --quiet`; BR-001, BR-002, BR-003 y BR-005.
- PASS: `cargo test --locked --offline ffmpeg_smoke_records_playable_timed_clip -- --ignored`; BR-004, duración del clip, decodificación sin tráiler y lectura tras terminar forzosamente FFmpeg.
- PASS: formato, Clippy con `-D warnings`, documentación con `RUSTDOCFLAGS=-D warnings`, inventario de release y compilación optimizada.
- NO EJECUTADO: una sesión real de varias horas contra Chaturbate, Windows/macOS y compatibilidad con editores externos.

Se preserva la estructura JSON v1 y se amplía `classification` con `interrupted`.
Los consumidores que restringen ese campo a `saved`/`small` deben admitir el nuevo
estado. Los archivos anteriores y los cambios locales previos no se migran ni eliminan.
