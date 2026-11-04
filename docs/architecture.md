# Organización del código

cbrec conserva un único crate con cuatro módulos públicos. El mapa de
[áreas de mantenimiento](../MAINTAINERS.md) agrupa las funcionalidades que
atraviesan esas carpetas; no requiere otro crate ni una rama por subsistema.

| Capa | Responsabilidad actual | Dependencias internas observadas |
|---|---|---|
| `domain` | Tipos validados y contrato `StreamRepository` | El propio dominio |
| `infrastructure` | HTTP, HLS, proceso FFmpeg, configuración y sesión local | Dominio y otros adaptadores |
| `presentation` | Argumentos CLI, salida, estado visual, entradas y miniaturas de terminal | Dominio y otros módulos de presentación |
| `application` | Comandos, grabación, vigilancia y coordinación de la TUI | Dominio, infraestructura y presentación |

`src/main.rs` construye configuración, cliente y argumentos y llama a
`application::ejecutar_cli`. La aplicación utiliza adaptadores concretos;
`Output` y los eventos de TUI viven en presentación. Esta es la estructura
existente, no una separación completa por interfaces entre todas las capas.

## Recorridos que debe preservar un cambio

- Grabación: comando CLI/TUI o vigilancia → `application::recording` →
  `StreamRepository::download_stream` → `ChaturbateClient` → `hls_relay` →
  `ffmpeg_process`. La aplicación gestiona el resultado y los metadatos;
  los adaptadores gestionan la red y el proceso.
- Descubrimiento: acciones de `presentation::tui` → `application::commands::tui`
  y `discover` → cliente HTTP → eventos de vuelta a la TUI.
- Miniaturas: descarga en infraestructura → decodificación y caché en
  `application::thumbnail` → representación en `presentation::thumbnail`.

Mantén las reglas y los tipos del dominio sin dependencias de red, terminal o
procesos. Coordina las operaciones en aplicación; deja HTTP y FFmpeg en
infraestructura y el dibujo en presentación. Antes de introducir una dependencia
en sentido contrario, explica el contrato y sus consumidores en el PR.

## Evolución por parches

1. Localizar el área y seguir sus llamadas, pruebas, configuración y exports.
2. Corregir un comportamiento con una regresión reproducible; medir las mejoras
   de rendimiento antes de afirmarlas.
3. Si hace falta extraer código, hacerlo en otro parche que conserve el
   comportamiento y los límites de visibilidad. El número total de líneas
   incluye pruebas y no determina por sí solo una extracción.
4. Al mover módulos, revisar también los ejemplos que incluyen fuentes mediante
   `#[path]` o `include!`, además de los scripts de QA y los imports.
5. Integrar y estabilizar según el [flujo de desarrollo](development.md).

La TUI concentra estado visual y gestión de entradas; sería una candidata a
separación si un cambio concreto necesita aislar esas responsabilidades. Esa
extracción está pendiente, no se ha ejecutado como parte de este mapa.

Se adoptan del [proceso de Linux](https://www.kernel.org/doc/html/latest/process/2.Process.html)
las responsabilidades por subsistema y la prioridad de las regresiones, y de su
[guía de parches](https://www.kernel.org/doc/html/latest/process/submitting-patches.html)
los cambios pequeños y justificables. Las carpetas, los plazos y el flujo de
revisión se ajustan a cbrec.
