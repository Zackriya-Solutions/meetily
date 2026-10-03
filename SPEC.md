# Sottoly — Spec

> Tu junta asesora en cada reunión. Se pronuncia **SÓ-to-li** (de *sotto voce*, "en voz baja").

Este documento guía la construcción de Sottoly. Lo leen, en orden, Claude Code y el autor. Reglas de lectura:

- Las decisiones están en imperativo. Si algo no está aquí, no se construye.
- Lo pendiente vive solo en [Preguntas abiertas](#12-preguntas-abiertas). No implementar nada de esa sección por iniciativa propia.
- El vocabulario es el de [GLOSSARY.md](GLOSSARY.md). Las decisiones difíciles de revertir están en [docs/adr/](docs/adr/).

---

## 1. Qué es

Sottoly ayuda a fundadores y CTOs a decidir mejor en sus reuniones 1:1 de alto riesgo (plata, contratos, plazos) sin pagar un asesor que los acompañe en cada reunión.

Una **Junta** de **Roles** (CFO, CEO adversarial…) escucha la **Reunión** y muestra al **Usuario** una **Sugerencia** corta en vivo: qué preguntar, qué revisar, qué no aceptar todavía. No toma notas para después: actúa durante la Reunión.

**Usuario del MVP:** fundador o CTO de startup o pyme en LatAm, habla español, usa Mac, hace reuniones por Meet, Zoom o Teams donde se decide plata, y se siente cómodo instalando una App. El autor es el usuario cero. "Horizontal" (consultores, abogados, vendedores) es la visión del formato de Roles, no del MVP.

---

## 2. Forma del producto

- **App** de escritorio (Tauri, macOS Apple Silicon primero). Corre en la máquina del Usuario: captura, transcripción local, Motor y overlay.
- **Motor** (`engine/`): sidecar TypeScript compilado con Bun a un solo binario. Recibe Segmentos, arma Turnos y produce Sugerencias.
- **Cloud** (v1, repo privado `sottoly-cloud`): cuenta, suscripción, historial de Decisiones, sync. No entra al MVP.
- **Trae tu propio modelo (BYOM):** el Usuario conecta sus propias keys. Sottoly no revende tokens en la versión gratuita.

### Open core

| Abierto (`sottoly`, este repo) | Cerrado |
|---|---|
| App, captura, transcripción, Motor, Compuerta, formato de Roles, Roles base, runner de evals con fixtures sintéticos | `sottoly-cloud` (Cloud), `sottoly-evals` (datos reales), `sottoly-roles-pro` (Roles premium) |

La App abierta es completa y gratuita con BYOM. Cloud cubre lo que exige infraestructura: modelos incluidos sin keys, historial, sync, Roles premium y add-ons.

---

## 3. Base técnica

| Pieza | Fuente | Licencia | Qué da |
|---|---|---|---|
| Captura + transcripción | [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes), `frontend/src-tauri` | MIT | Micrófono (`audio/capture/microphone.rs`) y audio del sistema (`audio/capture/system.rs`, CoreAudio process tap vía `cidre`) por separado, sin BlackHole. Whisper y Parakeet locales. App Tauri 2 + Next.js armada |
| Overlay | [Coucou](https://github.com/Louis-CFM/coucou) | MIT | Patrón de ventana flotante en el notch que no roba el foco |
| Motor | Propio | MIT | Turnos, Compuerta, Redacción, Verificación, Roles |

**Descartados:** Pluely y cheating-daddy (GPL-3), screenpipe (licencia comercial), Hyprnote (demasiado grande; se pueden tomar crates puntuales como AEC), Avibe (no es tiempo real).

Este repo es un fork fiel de Meetily ([ADR-0001](docs/adr/0001-fork-fiel-de-meetily.md)): no mover ni borrar carpetas de Meetily.

---

### Aprovechar Meetily

Todo se construye reutilizando el código de Meetily ([ADR-0001](docs/adr/0001-fork-fiel-de-meetily.md)), sin duplicar pipelines.

| Feature de Meetily | Uso en Sottoly |
|---|---|
| Parakeet "Lightning" (tiempo real) | STT predeterminado del MVP. La latencia de Segmentos (Q31) se mide con Lightning. "Compact" queda como opción para Macs con menos recursos. |
| Modelos locales de resumen (Qwen 3.5 4B/2B, Gemma 3 4B/1B, offline) | Decisiones al cierre (§6): reutilizar el pipeline de resumen de Meetily con un prompt de Sottoly que devuelva `decision` y `commitment`. Modo privado y plan sin keys: proveedor local en `ModelProvider`. Experimento: Gemma 3 1B como Compuerta local de respaldo, comparada con Jev sobre los mismos fixtures. |
| Auto Summary y Summary Language | Base para el resumen automático al cerrar la Reunión y para fijar el español como idioma por defecto. |
| Import Audio & Retranscribe (beta) | Evals: grabaciones reales con consentimiento → transcripciones → fixtures etiquetados en `sottoly-evals`. Demo de respaldo: reproducir una Reunión grabada por el pipeline completo. Pendiente: verificar si la retranscripción conserva Usuario / Contraparte o lo mezcla. |

Privacidad: Meetily guarda por defecto el audio (`auto_save: true`) y las transcripciones (SQLite). Sottoly no guarda ninguno de los dos por defecto (§7). El audio se apaga en el MVP; las transcripciones se quedan hasta después del Build Day (§11).

---

## 4. Arquitectura

```
┌──────────────────────── App (Tauri, fork de Meetily) ────────────────────────┐
│ Micrófono ─► Usuario     ┐                                                    │
│                          ├─► VAD + STT local (Parakeet/Whisper) ─► Segmentos  │
│ Sistema   ─► Contraparte ┘                                            │       │
│                                       engine_bridge.rs (stdio JSONL)  │       │
│  ┌──────────────────────── Motor (sidecar, Bun) ──────────────────────▼────┐  │
│  │ Segmentos ─► Turnos ─► Compuerta ─► Redacción ─► Verificación ─► Sugerencia │
│  │                        (Jev Choice) (Sonnet)    (Jev Score, v1)          │  │
│  └────────────────────────────────────────────────────────┬─────────────────┘  │
│                                                           ▼                    │
│                                     Overlay: tarjeta "Betty · CFO"             │
└────────────────────────────────────────────────────────────────────────────────┘
          Al cerrar la Reunión: Decisiones candidatas ─► revisión ─► Memoria local
```

### Latencia

Objetivo: fin de un Turno de la Contraparte → Sugerencia visible en **< 2 s en el p90**.

| Tramo | Presupuesto |
|---|---|
| Chunk de audio + VAD | 300–600 ms |
| STT local (Parakeet en Apple Silicon) | 200–400 ms |
| Compuerta (Jev) | 70–500 ms |
| Redacción (Sonnet, streaming) | 500–800 ms |
| Overlay | < 50 ms |

### Protocolo App ↔ Motor (`engine/src/protocol.ts`)

Una línea JSON por mensaje. Claves, enums y tipos en inglés; el contenido humano en el idioma de la Reunión.

```jsonc
// app → engine
{ "type": "segment", "speaker": "user" | "counterpart" | "mixed", "text": "...", "t0": 125.3, "t1": 128.6 }
{ "type": "session", "event": "start" | "end", "roles": ["cfo", "ceo"] }
{ "type": "clock", "t": 131.2 }            // solo si la medición de la Fase 1 lo exige
// engine → app
{ "type": "suggestion", "role": "cfo", "persona": "Betty",
  "text": "Pregunta si ese valor incluye IVA.",
  "reason": "Mencionó un precio sin aclarar impuestos.", "confidence": 0.82 }
{ "type": "summary", "decisions": [ /* Decision[] */ ] }
```

- `speaker: "mixed"`: cuando no se pueden separar los canales (plan de recorte o reunión presencial). La Compuerta no asume quién dijo qué.
- `counterpart_id` (c1, c2…) llega en v1 con diarización, sin cambiar `speaker`.

### Turnos

El Motor arma los Turnos a partir de los Segmentos. La App no emite Turnos.

- Cerrar el Turno con un hueco ≥ 700 ms entre el `t1` de un Segmento y el `t0` del siguiente, o con un cambio de `speaker`.
- Evaluar la Compuerta al cerrar un Turno de la Contraparte, al cerrar un Turno del Usuario, y cada 10 s de habla continua de un mismo hablante (sin cerrar el Turno).
- Implementar la lógica de Turnos como función pura sobre Segmentos (y latidos `clock`), probada con fixtures sin audio.
- Si llega a hacer falta el latido `clock`, emitirlo desde `engine_bridge.rs` (archivo nuevo), nunca editando Meetily.

### Compuerta

- Elegir entre **Roles, nunca Personas**. Opciones = `gate_option` de cada Rol activo, más `none`. MVP: `[none, cfo, ceo_adversarial]`.
- Máximo un Rol por evaluación. Pasa solo si la probabilidad supera el `threshold` del Rol.
- La Persona nunca entra al prompt de la Compuerta.
- El Motor traduce `gate_option` → id del Rol al recibir la respuesta; nada fuera de la Compuerta ve `gate_option`.
- Jev sigue el **nombre** de la opción más que su definición (arXiv 2609.26758): un `gate_option` que no calce con su `gate_definition` desvía las decisiones. Por eso el nombre es parte del criterio y cambiarlo exige recalibrar.

### Redacción

- Una Sugerencia = máximo 15 palabras + motivo en una línea.
- Streaming, `max_tokens` bajo, prompt caching del prompt de cada Rol.
- La Persona solo afecta la Redacción (tono) y la UI (nombre en la tarjeta).

### Antiruido

- Una sola Sugerencia visible a la vez.
- Mínimo 30 s entre Sugerencias del mismo Rol.
- No repetir una Sugerencia ya mostrada.
- Tope de Sugerencias por Reunión.

### Modelos (`engine/config.json`)

Los modelos viven en la configuración del Motor, nunca en los archivos de Rol ni en el código. IDs exactos, sin alias que cambien solos.

```jsonc
{
  "gate":          { "provider": "jev",       "model": "jev-1.13.0" },
  "draft":         { "provider": "anthropic", "model": "claude-sonnet-5-5", "max_tokens": 80 },
  "gate_fallback": { "provider": "anthropic", "model": "claude-haiku-4-5-20251001" }
}
```

- Implementar `ModelProvider` con el Vercel AI SDK: Anthropic (`@ai-sdk/anthropic`) y Jev (`@ai-sdk/typesafe-ai`, función `experimental_evaluate()`; Choice admite 1–255 opciones, Score 2–10 niveles). OpenAI, OpenRouter y Ollama quedan detrás de la misma interfaz, sin UI en el MVP.
- "Sonnet" en este documento significa `claude-sonnet-5-5`. Actualizar un ID es una decisión explícita.
- Si Sonnet pone en riesgo los 2 s, probar Haiku 4.5 también para la Redacción (cambiar una línea de config).

### Distribución del Motor

- `bun build --compile` a un binario por arquitectura, declarado en `externalBin` de Tauri con sufijo de arquitectura: `sottoly-engine-aarch64-apple-darwin`.
- MVP: solo Apple Silicon. Intel (`x86_64-apple-darwin`) en v1 si un piloto lo necesita.
- Al notarizar (después del Build Day): firmar el sidecar con la misma identidad Developer ID y revisar entitlements de hardened runtime para el JIT de Bun (`com.apple.security.cs.allow-jit`, posiblemente `allow-unsigned-executable-memory`).
- Plan B si Bun falla como sidecar: Deno compile; luego Node SEA.

---

## 5. Roles

Un Rol es un archivo Markdown con frontmatter YAML en `roles/`. Claves en inglés; valores de texto (`objective`, `gate_definition`, instrucciones) en el idioma del Rol; `limits` y `sources` son categorías en inglés.

| Campo | Ejemplo | Uso | ¿Cambia libremente? |
|---|---|---|---|
| id (nombre del archivo, snake_case, único) | `ceo` | Protocolo, evals (`expected_role`), Memoria, config | No |
| `gate_option` | `ceo_adversarial` | Solo la Compuerta | No: exige recalibrar |
| `role` | `CEO adversarial` | UI, tarjetas | Sí |
| `persona` | `Sheldon` | UI, tono de la Redacción | Sí |

```markdown
---
role: CFO
persona: Betty
objective: Proteger la caja y evitar compromisos con costos ocultos.
gate_option: cfo
gate_definition: Interviene cuando se mencionan cifras, precios, impuestos o condiciones de pago sin aclarar.
limits: [legal opinions, personal topics]
sources: [finance]
threshold: 0.75
calibrated_with: none
status: active
---
Instrucciones del rol en lenguaje natural (tono y criterio de la Redacción).
```

```markdown
---
role: CEO adversarial
persona: Sheldon
objective: Evitar decisiones por inercia; cuestionar supuestos y exigir evidencia.
gate_option: ceo_adversarial
gate_definition: Interviene cuando se acepta un acuerdo sin datos, aparece una contradicción o nadie menciona un riesgo evidente.
limits: [personal attacks, legal opinions]
sources: [decisions, projects]
threshold: 0.85
calibrated_with: none
status: active
---
Instrucciones del rol en lenguaje natural.
```

- `status`: `active` | `experimental` | `deprecated`. MVP: `cfo` y `ceo` activos; `cto` y `cmo` existen como `experimental`, desactivados y fuera de la Compuerta.
- Validar cada archivo con un esquema Zod al arrancar el Motor y en CI: claves obligatorias, id único, `status` válido.
- Regla de recalibración: si cambia `gate_option` o `gate_definition` y no cambia `calibrated_with`, el check falla con "Recalibra este Rol con evals".
- Roles creados por usuarios: `calibrated_with: none`, la App los marca "sin calibrar" y usan umbral 0.9 por defecto.
- `sources` son categorías genéricas. Cada Usuario las mapea a sus carpetas de Notas en `~/.sottoly/sources.yaml`, fuera del repo.
- Las Personas se describen solo por su criterio y tono propios. Sin referencias a personajes de ficción, ni en texto ni en imágenes.

---

## 6. Decisiones y Memoria

- Al cerrar la Reunión, el Motor propone Decisiones candidatas (`summary`). El Usuario aprueba, edita o descarta cada una, o elige "no guardar nada".
- Solo lo aprobado entra a la Memoria. La transcripción se descarta.
- Marca en vivo (atajo global "marcar decisión"): guarda la marca de tiempo; al cierre el Motor redacta la Decisión con los ~60 s alrededor y la preselecciona.
- Las preguntas abiertas no se guardan en el MVP.

```jsonc
{ "id": "...", "kind": "decision" | "commitment",
  "text": "Se contrata el plan anual del software contable.",
  "owner": "user" | "counterpart",
  "due": null,                       // fecha si es commitment
  "source": "engine" | "live_mark",
  "meeting_id": "...", "created_at": "...", "approved": true }
```

- MVP: Memoria en archivos locales, `~/.sottoly/memory/`, un Markdown por Reunión, con opción de exportar a las Notas del Usuario.
- v1: Memoria en Cloud (Postgres + pgvector) para sync, búsqueda y para que los Roles consulten Decisiones anteriores.

---

## 7. Privacidad y consentimiento

- No guardar audio. No guardar transcripción por defecto; el ajuste "guardar transcripción local" viene apagado.
- Métricas solo locales y sin contenido: fecha, duración, Roles activos, número de Sugerencias, útil / no útil. Sin telemetría.
- Mostrar un aviso de consentimiento al iniciar la sesión: texto corto en español que el Usuario lee o pega en el chat de la Reunión.
- El overlay **sí** aparece al compartir pantalla. No copiar el modo "invisible" de otros asistentes: es el diferencial ético de Sottoly.
- Nada sale por voz sin un toggle explícito del Usuario.
- Las keys van en el Keychain de macOS, nunca en disco ni en git.
- Ley 1581 de 2012 (Colombia): validar con un abogado antes de vender.

---

## 8. Evals

| `sottoly/evals/` (público) | `sottoly-evals` (privado) |
|---|---|
| `runner/`, `fixtures/` con Reuniones **sintéticas** en español y sus etiquetas, `FORMAT.md` | Transcripciones reales con consentimiento, etiquetas, resultados por versión |

- Nada que venga de una Reunión real llega al repo público, ni anonimizado. Los fixtures sintéticos se escriben a partir de patrones, no copiando fragmentos.
- El runner lee el dataset privado con `SOTTOLY_EVALS_DIR`.

Etiqueta por Turno:

```jsonc
{ "turn": 42, "speaker": "counterpart", "text": "...",
  "should_intervene": true, "expected_role": "cfo",
  "reason": "precio sin aclarar IVA" }
```

Métricas: precisión y recall de la Compuerta, acierto del Rol elegido, Sugerencias por hora, latencia p90 de punta a punta.

Prueba de cordura: correr los fixtures con la Persona renombrada (Betty → Ana) y exigir decisiones idénticas.

---

## 9. Pruebas y PRs

Un PR se integra solo si pasan todos los checks requeridos. Ninguna tarea está terminada sin prueba: escribir o actualizar la prueba, implementar, correr, iterar hasta verde, abrir el PR.

| Capa | Herramienta | Qué prueba | ¿Bloquea PR? |
|---|---|---|---|
| Unitarias del Motor | `bun test` | Armado de Turnos, mapeo `gate_option` → id, esquema de Roles (Zod), protocolo JSON | Sí |
| Unitarias Rust | `cargo test` | Separación Usuario / Contraparte, `engine_bridge.rs` | Sí |
| Contrato | `bun test` + respuestas grabadas | Turno → Compuerta → Redacción con Jev y Sonnet grabados (sin keys, determinista) | Sí |
| E2E del Motor | `bun test` sobre fixtures | Transcripción sintética entra, Sugerencias esperadas salen (sin audio, sin UI) | Sí |
| E2E de UI | Playwright | Overlay y Ajustes como página web con el IPC de Tauri simulado (`mockIPC`): llega una Sugerencia → aparece la tarjeta; "útil / no útil" funciona | Sí |
| Calidad de la Compuerta | Runner de evals con modelos reales | Precisión, recall, latencia | No: es un informe, no es determinista |

- Playwright no maneja la App nativa: en macOS Tauri usa WKWebView y no hay driver WebDriver. La integración nativa se cubre con `cargo test`, el E2E del Motor y la prueba manual de la Reunión real.
- Claude Code usa el MCP de Playwright para abrir el overlay, mirar el resultado y corregir. Lo que se prueba a mano se convierte en spec de Playwright.
- No desactivar, borrar ni marcar como skip una prueba para que pase. Si una prueba está mal, explicarlo en el PR.

**CI (GitHub Actions):** en cada PR, `bun test`, `cargo test`, Playwright, gitleaks y validación de Roles. Runner de macOS solo para el build de Tauri y `cargo test`; el resto en Linux.

**Branch protection en `main`:** checks requeridos, sin push directo, PR obligatorio.

**Alcance del fin de semana:** viernes, CI con `bun test` + gitleaks + branch protection; sábado, unitarias, contrato y E2E del Motor con los 5 fixtures a medida que se construye; domingo, 2–3 specs de Playwright del overlay. Después del Build Day: `cargo test` en CI con macOS, evals con modelos reales.

---

## 10. Plan: Build Day (lunes 5 de octubre de 2026)

**Objetivo único:** en una Reunión real, Betty muestra al menos una Sugerencia útil en menos de 2 s, en el overlay.

| Cuándo | Qué | Listo cuando | Corte |
|---|---|---|---|
| Vie 2, noche | Fork compilando y transcribiendo en el Mac (`cd frontend && pnpm install && pnpm run tauri:dev`). Prueba Bun: "hola Jev" compilado, lanzado como sidecar en modo desarrollo | Una Reunión de prueba produce Segmentos; el sidecar responde una línea JSON con la decisión de Jev | Si Bun falla como sidecar: Motor con `bun run` local, sin compilar |
| Sáb 3, mañana | Separar Usuario / Contraparte antes del mezclador (`audio/pipeline.rs`); agregar `speaker` a `TranscriptUpdate` (`audio/transcription/worker.rs`); medir latencia de Segmentos | Una Reunión de 10 min transcribe Usuario y Contraparte por separado; p90 de llegada de Segmentos medido | 14:00: si no funciona → `speaker: "mixed"` |
| Sáb 3, tarde | `engine/`: Turnos, Compuerta con Jev (Betty), Redacción con Sonnet; 5 fixtures sintéticos | Los 5 fixtures corren por el Motor y producen Sugerencias en los Turnos etiquetados | — |
| Sáb 3, noche | Overlay: tarjeta con Persona, Rol, Sugerencia y motivo; botones "útil / no útil"; se desvanece a los ~12 s; atajo global para silenciar | Una Sugerencia del Motor aparece en el overlay sin robar el foco | Si se complica: ventana flotante normal |
| Dom 4, mañana | Sheldon + ajuste de umbrales con los fixtures | Sheldon interviene en sus Turnos etiquetados sin meter ruido en los de Betty | 12:00: si mete ruido → demo solo con Betty |
| Dom 4, tarde | Reunión de prueba real con consentimiento; revisión de Decisiones al cierre si sobra tiempo | Latencia p90 < 2 s en 10 Sugerencias seguidas | — |
| Dom 4, noche | Congelar código, README corto, video de respaldo del demo, ensayo | — | Nada nuevo después de las 21:00 |
| Lun 5 | Arreglos menores, demo en vivo + video de respaldo | — | — |

**Reglas para tocar código de Meetily** ([ADR-0001](docs/adr/0001-fork-fiel-de-meetily.md)):

- Preferir archivos nuevos: `engine_bridge.rs`, `overlay/`, `speaker.rs`.
- Ediciones mínimas donde no hay otra opción (agregar `speaker`, registrar el puente y la ventana overlay, nombre e ícono en `tauri.conf.json`), marcadas con `// SOTTOLY:`.
- No borrar carpetas ni proveedores (`openrouter`, `ollama`); lo que no se usa se excluye del build.
- No hacer merge de `upstream` antes del Build Day.

### Fuera de alcance del MVP

No construir antes del Build Day:

- Verificación (Check) y respaldo de la Compuerta con Haiku.
- `cargo test` en CI, Playwright en CI y runner de evals con modelos reales (quedan los fixtures + `bun test`).
- Firma, notarización y pilotos.
- Marca de Decisión en vivo.
- Roles CTO y CMO activos, Abogado, Due diligence.
- Cloud, landing con lista de espera, login de la App.
- Memoria en Cloud, consulta de Decisiones anteriores, Notas por MCP.
- Bot en Meet/Teams, voz, transcripción en la nube.
- UI para OpenAI, OpenRouter, Ollama.

---

## 11. Después del Build Day

| Fase | Contenido |
|---|---|
| Pilotos | Firma y notarización, 5 pilotos fundadores, Ajustes → Modelos ("Key de Anthropic", "Key de Jev", "Probar conexión", Keychain), Verificación, respaldo con Haiku, CI completo (`cargo test` en macOS, Playwright) + job nocturno de evals con modelos reales |
| v1 | Roles CTO, CMO, Abogado y Due diligence (datos públicos de gobierno por MCP); Notas por MCP en solo lectura; Memoria y login en Cloud; `counterpart_id` con diarización; Compuerta local con modelos abiertos tipo Jev (Laya, Open-Jev); Intel |
| v1.1 | Transcripción en la nube opcional (Deepgram) implementando `TranscriptionProvider` |
| v2 | Bot en Meet y Teams, voz con toggle explícito, Roles compartibles |

**Privacidad (primera tarea después del Build Day):**

- Apagar por defecto el guardado de transcripciones en SQLite que hereda Meetily; activarlo a mano solo para evals con consentimiento.
- Un comando o botón para borrar las transcripciones de prueba guardadas durante el Build Day.

**Criterio para pasar de fase:** el Usuario usa una Sugerencia en al menos 1 de cada 3 Reuniones. Con pilotos: al menos 3 de 5 usan Sottoly en más de una Reunión real.

**Agent SDK:** solo para Roles con herramientas (Due diligence, Notas), fuera de la ruta caliente. Alternativa a evaluar: herramientas MCP desde el AI SDK.

---

## 12. Preguntas abiertas

No implementar nada de esta sección sin una decisión explícita.

- Línea base de la Compuerta (2026-10-02, `jev-1.13.0`, 5 fixtures sintéticos, 25 Turnos): precisión 0,60, recall 0,55, acierto de Rol 1,00, latencia p50 231 ms / p90 286 ms. El CEO adversarial no supera su umbral en ningún cierre apresurado, y el CFO repite en Turnos posteriores porque la ventana de 90 s conserva la cifra. ¿La Compuerta debe evaluar sobre todo el último Turno? Se decide con evals antes de tocar prompts.
- Límites de uso del early access de Jev (peticiones por minuto), keys separadas por piloto, y si sus términos permiten publicar comparativas.
- ¿El VAD de Meetily cierra chunks en los silencios con la latencia suficiente para no necesitar el latido `clock`? Se responde con la medición del sábado.
- Entitlements exactos de hardened runtime para el binario de Bun.
- Login de la App contra Cloud con PKCE y deep link `sottoly://auth/callback` en Tauri: no hay guía oficial; probar antes de v1.
- Validación legal del consentimiento (Ley 1581 de 2012).

---

## Créditos

- Captura y transcripción basadas en [Meetily](https://github.com/Zackriya-Solutions/meeting-minutes) (MIT, © Zackriya Solutions).
- Patrón de overlay inspirado en [Coucou](https://github.com/Louis-CFM/coucou) (MIT).
