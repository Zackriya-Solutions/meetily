'use client';

import { forwardRef, type HTMLAttributes } from 'react';
import type { PaneResizeHandleProps } from '@/hooks/usePaneResize';

interface Props extends Omit<HTMLAttributes<HTMLDivElement>, keyof PaneResizeHandleProps> {
  handleProps: PaneResizeHandleProps;
  label: string;
  valueNow: number;
  valueMin: number;
  valueMax: number;
  valueText: string;
  /** Classes for the hit area, including its position and display (e.g. `relative hidden md:flex`). */
  className?: string;
}

/**
 * The vertical resize handle between two panes: an 8px hit area around a 1px
 * line that thickens on hover and while dragging. Focusable, arrow keys resize,
 * double-click resets.
 */
export const PaneResizeHandle = forwardRef<HTMLDivElement, Props>(function PaneResizeHandle(
  { handleProps, label, valueNow, valueMin, valueMax, valueText, className = 'relative flex', ...rest },
  ref,
) {
  return (
    <div
      ref={ref}
      role="separator"
      aria-orientation="vertical"
      aria-valuenow={valueNow}
      aria-valuemin={valueMin}
      aria-valuemax={valueMax}
      aria-valuetext={valueText}
      aria-label={label}
      title={`${label} (double-click to reset)`}
      tabIndex={0}
      {...rest}
      {...handleProps}
      className={`group z-10 w-2 flex-shrink-0 cursor-col-resize touch-none select-none items-stretch justify-center focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-inset ${className}`}
    >
      <div className="h-full w-px bg-gray-200 transition-[width,background-color] duration-150 ease-out group-hover:w-1 group-hover:bg-blue-400 group-active:w-1 group-active:bg-blue-500" />
    </div>
  );
});
