# Registro de cambios

## v0.0.3 · 2026-11-05

- TUI con Inicio que carga el catálogo del sitio, búsqueda por URL o `@usuario`, tags y páginas navegables.
- Conexión de sesión guiada por navegador, instrucciones de sesión abierta y cancelación del receptor.
- Búsquedas cancelables, plazo de ocho segundos y conservación de resultados ante errores.
- Miniaturas con caché HTTP y píxeles nativos cuando el terminal confirma soporte gráfico.
- Recuperación de segmentos HLS truncados con reintentos y memoria acotados, sin recodificar.
- Conservación de grabaciones, metadatos y diagnósticos finales ante fallos y cierre de procesos.
- Validaciones de red y sesión, límites de recursos y regresiones de concurrencia.
- CI con cobertura mínima del 90 %, QA de terminal y comprobación de artefactos.
- Flujo `beta` → `main`, tags preliminares y metadatos públicos bajo Duna-tools.

Los reintentos no reparan una fuente dañada ni garantizan recuperar interrupciones
permanentes. No se han verificado emisiones de horas ni todos los protocolos
gráficos nativos. Consulta los límites en [recuperación](docs/recording_recovery.md)
y [miniaturas](docs/thumbnail_rendering.md).

Las versiones publicadas se consultan en [Releases](https://github.com/Duna-tools/cbrec/releases).
