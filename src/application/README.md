# Application Layer

Responsabilidad: orquestar la CLI y ejecutar los casos de uso de grabacion.

- Concurrencia: limite configurable con `--jobs` (por defecto 3).
- Salida: <raiz>/cb_rec/<modelo>/<timestamp>_<modelo>.mp4
- Dependencia: ffmpeg debe estar en PATH o usar `--ffmpeg-path`.
- Cancelacion: Ctrl+C detiene todas las grabaciones.
- Modelos duplicados se ignoran.
- Logs compactos cuando hay multiples grabaciones.
