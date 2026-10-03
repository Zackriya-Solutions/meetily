# ADR-0001: Fork fiel de Meetily, sin podar

Estado: aceptada · Fecha: 2026-10-02

## Contexto
Sottoly parte de Meetily (MIT) para captura de audio nativa en macOS y
transcripción local. Se eligió un fork de GitHub para seguir trayendo
mejoras de upstream (captura, Whisper, Parakeet).

## Decisión
Mantener el layout de Meetily sin mover ni borrar carpetas. El código de
Sottoly vive en carpetas nuevas en la raíz (`engine/`, `roles/`, `evals/`,
`docs/`). Lo que no se usa (`backend/`, `llama-helper/`, proveedores
`openrouter` y `ollama`) se excluye del build, no se borra. Las ediciones
a archivos de Meetily son mínimas y se marcan con `// SOTTOLY:`.

## Alternativas
- Reestructurar a `app/` y podar: repo más limpio, pero cada merge de
  upstream se vuelve conflictos de renombre y borrado.
- Copiar el código sin fork: sin vínculo con upstream; se pierden mejoras.

## Consecuencias
- (+) Merges de upstream baratos.
- (−) Carpetas sin uso en el repo; hay que explicarlas en el README.
- Revisar en v1: si los cambios en `frontend/` hacen que los merges
  duelan más de lo que ayudan, se corta el vínculo y se reestructura.
