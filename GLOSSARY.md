# Sottoly

Sottoly es una junta asesora de IA que escucha las reuniones del usuario y le muestra sugerencias en vivo para decidir mejor. Este glosario fija el vocabulario del proyecto: úsalo siempre en código, UI y docs. Claves, enums y tipos van en inglés; el contenido humano (transcripción, sugerencia, motivo) va en el idioma de la reunión.

## Lenguaje

### App (App)
La aplicación de escritorio que corre en la máquina del usuario.
- En código: `app`
- No usar: cliente

### Cloud (Cloud)
El panel web y los servicios pagos de Sottoly.
- En código: `cloud`
- No usar: panel, dashboard

### Compuerta (Gate)
Decide si un rol habla tras un Turno, y cuál; elige entre Roles, nunca entre Personas.
- En código: `gate`, `GateDecision`
- No usar: filtro

### Contraparte (Counterpart)
Cualquier persona del otro lado de la reunión; su voz entra por el audio del sistema.
- En código: speaker `counterpart`, `counterpart_id` (v1)
- No usar: ellos

### Decisión (Decision)
Algo resuelto o comprometido en una Reunión que el Usuario aprueba guardar en la Memoria.
- En código: `Decision`, `kind: decision | commitment`
- No usar: acuerdo, conclusión, minuta

### Jev (Jev)
Modelo de decisión de TypeSafe AI usado como proveedor de la Compuerta y de la Verificación.
- En código: provider `jev`
- No confundir con: la Compuerta, que es el componente; Jev es un proveedor intercambiable

### Junta (Board)
Conjunto de roles activos en una reunión.
- En código: `board`
- No usar: panel

### Memoria (Memory)
Lo que Sottoly guarda de las reuniones: solo las Decisiones que el Usuario aprueba.
- En código: `memory`
- No usar: cerebro, second brain

### Motor (Engine)
La parte de Sottoly que recibe Turnos y produce Sugerencias: Compuerta, Redacción, Verificación y roles.
- En código: `engine`
- No usar: cerebro, brain

### Notas (Notes)
Sistema externo del usuario (Obsidian, gbrain u otro) que Sottoly consulta.
- En código: `notes`
- No usar: second brain, cerebro

### Persona (Persona)
Nombre, tono y voz de un rol (Betty, Sheldon); el usuario la puede cambiar sin tocar el criterio del rol.
- En código: `persona`
- No usar: rol (como sinónimo)

### Redacción (Draft)
Escribe la Sugerencia del rol elegido por la Compuerta.
- En código: `draft`, `Drafter`

### Reunión (Meeting)
Cualquier conversación que Sottoly escucha.
- En código: `meeting`
- No usar: llamada

### Rol (Role)
Cargo y criterio de un asesor: qué defiende, qué lo dispara, de qué no opina y su umbral.
- En código: `role`
- No usar: agente (como sinónimo)

### Segmento (Segment)
Fragmento de transcripción que emite la App; varios Segmentos forman un Turno.
- En código: `segment`

### Sugerencia (Suggestion)
Lo que un rol le muestra al usuario durante una reunión.
- En código: `suggestion`
- No usar: susurro, consejo (se permite "susurro" solo en marketing)

### Turno (Turn)
Tramo continuo de habla de un solo hablante; se cierra con un silencio o un cambio de hablante.
- En código: `Turn`, `turn.close`
- No usar: intervención (para el habla), segmento

### Usuario (User)
Quien usa Sottoly; su voz entra por el micrófono.
- En código: speaker `user`
- No usar: yo

### Verificación (Check)
Evalúa la Sugerencia redactada antes de mostrarla; si no pasa, se descarta.
- En código: `check`, `CheckResult`
- No usar: validación, filtro
