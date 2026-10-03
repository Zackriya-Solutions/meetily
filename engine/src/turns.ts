// Turnos: el Motor los arma a partir de los Segmentos (SPEC §4). La App no emite Turnos.
import type { Speaker } from "./protocol";
import type { WindowSegment } from "./gate";

export interface Turn {
  index: number;
  speaker: Speaker;
  segments: WindowSegment[];
  t0: number;
  t1: number;
  text: string;
}

/**
 * - `turn_closed`: el Turno terminó (hueco ≥ gap o cambio de hablante).
 * - `continuous`: habla continua ≥ continuousSeconds; evalúa la Compuerta sin cerrar el Turno.
 */
export type TurnEvent =
  | { kind: "turn_closed"; turn: Turn }
  | { kind: "continuous"; turn: Turn };

export interface TurnOptions {
  gapSeconds: number;
  continuousSeconds: number;
}

export const DEFAULT_TURN_OPTIONS: TurnOptions = { gapSeconds: 0.7, continuousSeconds: 10 };

export class TurnAssembler {
  private open: (Turn & { lastEvaluatedAt: number }) | null = null;
  private nextIndex = 0;

  constructor(private readonly options: TurnOptions = DEFAULT_TURN_OPTIONS) {}

  push(segment: WindowSegment): TurnEvent[] {
    const events: TurnEvent[] = [];
    const open = this.open;

    if (open && (segment.speaker !== open.speaker || segment.t0 - open.t1 >= this.options.gapSeconds)) {
      events.push(this.close());
    }

    if (!this.open) {
      this.open = {
        index: this.nextIndex++,
        speaker: segment.speaker,
        segments: [segment],
        t0: segment.t0,
        t1: segment.t1,
        text: segment.text,
        lastEvaluatedAt: segment.t0,
      };
    } else {
      this.open.segments.push(segment);
      this.open.t1 = Math.max(this.open.t1, segment.t1);
      this.open.text = `${this.open.text} ${segment.text}`;
    }

    const current = this.open!;
    if (current.t1 - current.lastEvaluatedAt >= this.options.continuousSeconds) {
      current.lastEvaluatedAt = current.t1;
      events.push({ kind: "continuous", turn: snapshot(current) });
    }
    return events;
  }

  /** Latido del reloj de audio: cierra el Turno si ya pasó el hueco desde su último Segmento. */
  tick(t: number): TurnEvent[] {
    if (this.open && t - this.open.t1 >= this.options.gapSeconds) return [this.close()];
    return [];
  }

  /** Cierra el Turno abierto (fin de la Reunión). */
  flush(): TurnEvent[] {
    return this.open ? [this.close()] : [];
  }

  private close(): TurnEvent {
    const turn = snapshot(this.open!);
    this.open = null;
    return { kind: "turn_closed", turn };
  }
}

function snapshot(turn: Turn & { lastEvaluatedAt?: number }): Turn {
  return {
    index: turn.index,
    speaker: turn.speaker,
    segments: [...turn.segments],
    t0: turn.t0,
    t1: turn.t1,
    text: turn.text,
  };
}
