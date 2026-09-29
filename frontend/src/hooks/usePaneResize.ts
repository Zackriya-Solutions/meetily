'use client';

import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';

/**
 * Drag, keyboard and persistence logic for a resize handle on the right edge of
 * a left-hand pane. The value can be anything the caller measures (pixels, a
 * ratio of the container); the caller maps pointer positions onto it.
 */
export interface PaneResizeOptions {
  /** localStorage key the committed value is persisted under. */
  storageKey: string;
  defaultValue: number;
  min: number;
  max: number;
  /** How far one arrow-key press moves the handle. */
  step: number;
  /** Map a pointer x coordinate to an unclamped value, or null to ignore the event. */
  valueFromPointer: (clientX: number) => number | null;
  /**
   * When set together with onCollapse, releasing a drag below this unclamped
   * value, or pressing ArrowLeft while already at `min`, collapses the pane.
   */
  collapseBelow?: number;
  onCollapse?: () => void;
}

export function clampValue(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function readStoredNumber(key: string, min: number, max: number, fallback: number): number {
  if (typeof window === 'undefined') return fallback;
  try {
    const raw = localStorage.getItem(key);
    const n = raw == null ? NaN : Number(raw);
    return Number.isFinite(n) && n >= min && n <= max ? n : fallback;
  } catch {
    return fallback;
  }
}

export function writeStoredValue(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Layout persistence is optional.
  }
}

/** Next value for a key press on the handle, 'collapse', or null when the key is not ours. */
export function nextValueForKey(
  key: string,
  current: number,
  { min, max, step, canCollapse }: { min: number; max: number; step: number; canCollapse: boolean },
): number | 'collapse' | null {
  switch (key) {
    case 'ArrowLeft':
      if (canCollapse && current <= min) return 'collapse';
      return clampValue(current - step, min, max);
    case 'ArrowRight':
      return clampValue(current + step, min, max);
    case 'Home':
      return min;
    case 'End':
      return max;
    default:
      return null;
  }
}

function canCollapse({ collapseBelow, onCollapse }: PaneResizeOptions): boolean {
  return collapseBelow != null && onCollapse != null;
}

export function usePaneResize(options: PaneResizeOptions) {
  const { storageKey, defaultValue, min, max } = options;
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const [value, setValue] = useState(defaultValue);
  const [isDragging, setIsDragging] = useState(false);
  const [collapseArmed, setCollapseArmed] = useState(false);
  const valueRef = useRef(defaultValue);
  const dragRef = useRef<{ startValue: number; armed: boolean } | null>(null);

  const apply = useCallback((next: number, persist: boolean) => {
    valueRef.current = next;
    setValue(next);
    if (persist) writeStoredValue(optionsRef.current.storageKey, String(next));
  }, []);

  useEffect(() => {
    apply(readStoredNumber(storageKey, min, max, defaultValue), false);
  }, [apply, storageKey, min, max, defaultValue]);

  const onPointerDown = useCallback((event: PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = { startValue: valueRef.current, armed: false };
    setIsDragging(true);
  }, []);

  const onPointerMove = useCallback((event: PointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const { valueFromPointer, collapseBelow, min: lo, max: hi } = optionsRef.current;
    const raw = valueFromPointer(event.clientX);
    if (raw == null) return;
    const armed = canCollapse(optionsRef.current) && raw < (collapseBelow as number);
    if (armed !== drag.armed) {
      drag.armed = armed;
      setCollapseArmed(armed);
    }
    apply(clampValue(raw, lo, hi), false);
  }, [apply]);

  const endDrag = useCallback((cancelled: boolean) => {
    const drag = dragRef.current;
    if (!drag) return;
    dragRef.current = null;
    setIsDragging(false);
    setCollapseArmed(false);
    if (cancelled) {
      apply(drag.startValue, false);
    } else if (drag.armed) {
      // Collapse keeps the width from before the drag, so expanding restores it.
      apply(drag.startValue, false);
      optionsRef.current.onCollapse?.();
    } else {
      apply(valueRef.current, true);
    }
  }, [apply]);

  const onPointerUp = useCallback(() => endDrag(false), [endDrag]);
  const onPointerCancel = useCallback(() => endDrag(true), [endDrag]);

  const onKeyDown = useCallback((event: KeyboardEvent<HTMLElement>) => {
    const { min: lo, max: hi, step } = optionsRef.current;
    const next = nextValueForKey(event.key, valueRef.current, { min: lo, max: hi, step, canCollapse: canCollapse(optionsRef.current) });
    if (next === null) return;
    event.preventDefault();
    if (next === 'collapse') {
      optionsRef.current.onCollapse?.();
      return;
    }
    apply(next, true);
  }, [apply]);

  const reset = useCallback(() => {
    apply(optionsRef.current.defaultValue, true);
  }, [apply]);

  return {
    value,
    isDragging,
    /** True while a drag is past `collapseBelow`: releasing now collapses the pane. */
    collapseArmed,
    reset,
    handleProps: {
      onPointerDown,
      onPointerMove,
      onPointerUp,
      onPointerCancel,
      onKeyDown,
      onDoubleClick: reset,
    },
  };
}

export type PaneResizeHandleProps = ReturnType<typeof usePaneResize>['handleProps'];
