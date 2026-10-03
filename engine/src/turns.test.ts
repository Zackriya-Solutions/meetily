import { describe, expect, test } from "bun:test";
import { TurnAssembler } from "./turns";
import type { WindowSegment } from "./gate";

const seg = (speaker: WindowSegment["speaker"], t0: number, t1: number, text = "x"): WindowSegment => ({
  speaker,
  text,
  t0,
  t1,
});

describe("TurnAssembler", () => {
  test("un hueco ≥ 700 ms cierra el Turno", () => {
    const a = new TurnAssembler();
    expect(a.push(seg("counterpart", 0, 2, "Son dos millones"))).toEqual([]);
    const events = a.push(seg("counterpart", 2.8, 4, "más IVA"));
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ kind: "turn_closed", turn: { index: 0, text: "Son dos millones" } });
  });

  test("un hueco < 700 ms no cierra el Turno y concatena el texto", () => {
    const a = new TurnAssembler();
    a.push(seg("counterpart", 0, 2, "Son dos millones"));
    expect(a.push(seg("counterpart", 2.5, 4, "más IVA"))).toEqual([]);
    expect(a.flush()[0]).toMatchObject({ turn: { text: "Son dos millones más IVA", t0: 0, t1: 4 } });
  });

  test("un cambio de hablante cierra el Turno aunque no haya hueco", () => {
    const a = new TurnAssembler();
    a.push(seg("counterpart", 0, 2));
    const events = a.push(seg("user", 2.1, 3));
    expect(events).toEqual([expect.objectContaining({ kind: "turn_closed", turn: expect.objectContaining({ speaker: "counterpart" }) })]);
  });

  test("habla continua ≥ 10 s dispara una evaluación sin cerrar el Turno", () => {
    const a = new TurnAssembler();
    a.push(seg("counterpart", 0, 4));
    a.push(seg("counterpart", 4.2, 8));
    const events = a.push(seg("counterpart", 8.3, 11));
    expect(events).toEqual([expect.objectContaining({ kind: "continuous" })]);
    // el siguiente Segmento sigue en el mismo Turno
    expect(a.push(seg("counterpart", 11.2, 12))).toEqual([]);
    expect(a.flush()[0]!.turn.segments).toHaveLength(4);
  });

  test("el latido cierra el Turno cuando ya pasó el hueco", () => {
    const a = new TurnAssembler();
    a.push(seg("counterpart", 0, 2));
    expect(a.tick(2.5)).toEqual([]);
    expect(a.tick(2.7)).toEqual([expect.objectContaining({ kind: "turn_closed" })]);
    expect(a.tick(5)).toEqual([]);
  });

  test("numera los Turnos en orden", () => {
    const a = new TurnAssembler();
    a.push(seg("counterpart", 0, 1));
    a.push(seg("user", 1.1, 2));
    a.push(seg("counterpart", 2.1, 3));
    expect(a.flush()[0]!.turn.index).toBe(2);
  });

  test("flush sin Turno abierto no emite nada", () => {
    expect(new TurnAssembler().flush()).toEqual([]);
  });
});
