# Formato de evals

Todo lo que hay en `evals/fixtures/` es **sintético**: reuniones escritas a partir de patrones, nunca copiadas de una Reunión real. Los datos reales viven en el repo privado `sottoly-evals` y el runner los lee con `SOTTOLY_EVALS_DIR`.

## Fixture (`fixtures/<id>.json`)

```jsonc
{
  "id": "contador-iva",
  "description": "Qué prueba esta Reunión",
  "roles": ["cfo", "ceo"],                 // ids de Rol activos en la Junta
  "segments": [                            // Segmentos tal como los emite la App
    { "speaker": "counterpart", "text": "...", "t0": 0, "t1": 4.2 }
  ],
  "labels": [                              // una etiqueta por Turno cerrado, en orden
    { "turn": 0, "speaker": "counterpart", "text": "...",
      "should_intervene": true, "expected_role": "cfo",
      "reason": "precio sin aclarar IVA" }
  ]
}
```

- `turn` es el índice del Turno que arma el Motor (hueco ≥ 700 ms o cambio de hablante).
- `expected_role` es el **id** del Rol, nunca la Persona; `null` cuando `should_intervene` es `false`.
- Claves en inglés; el texto en el idioma de la Reunión.

## Grabaciones (`recordings/<modelo>.json`)

Respuestas de la Compuerta grabadas contra el modelo real, indexadas por el hash de `(modelo, estado, pregunta)`. Las pruebas de contrato las reproducen sin keys. Si cambia el prompt de la Compuerta, un `gate_option` o una `gate_definition`, el hash cambia y la prueba falla con "missing recording": hay que volver a grabar.

```bash
cd engine
TYPESAFE_AI_API_KEY=... bun ../evals/runner/run.ts --record   # graba contra Jev y reporta métricas
bun ../evals/runner/run.ts                                     # reproduce y reporta métricas
```

## Métricas

Precisión y recall de "debía intervenir", acierto del Rol elegido y, en modo `--record`, latencia p50/p90 de la Compuerta. Son un informe: no bloquean PRs.
