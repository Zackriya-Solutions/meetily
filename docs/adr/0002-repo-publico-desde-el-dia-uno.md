# ADR-0002: Repo público desde el día uno

Estado: aceptada · Fecha: 2026-10-02

## Contexto
El motor de Sottoly es open source. Se evaluó mantenerlo privado hasta
el Build Day.

## Decisión
`sottoly` es público desde su creación. Datos reales, Cloud y estrategia
comercial viven en repos privados (`sottoly-evals`, `sottoly-cloud`).

## Alternativas
- Privado hasta el Build Day: más libertad para errores, pero un fork
  de GitHub de un repo público no puede ser privado y se pierde el
  contenido de construir en público.

## Consecuencias
- (+) Credibilidad en el Build Day, contenido y comunidad desde ya.
- (−) Todo commit es público e irreversible: obliga a pre-commit con
  gitleaks y lista de palabras bloqueadas, y a `CLAUDE.local.md`.
