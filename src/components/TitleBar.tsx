import { getCurrentWindow } from '@tauri-apps/api/window';
import { PhysicalPosition } from '@tauri-apps/api/dpi';
import { useCallback } from 'react';

export function TitleBar() {
  const onPointerDown = useCallback(async (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();

    // setPointerCapture before any await: gives OS-level mouse capture so
    // pointermove keeps firing even after the cursor leaves the window.
    // Without this, events stop at the window edge and drag breaks when
    // the window is already focused (native macOS activation no longer
    // holds implicit capture for us).
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);

    const win = getCurrentWindow();
    const startX = e.screenX;
    const startY = e.screenY;
    const factor = window.devicePixelRatio ?? 1;

    let base: Awaited<ReturnType<typeof win.outerPosition>> | null = null;
    let lastEv: PointerEvent | null = null;

    const onMove = (mv: PointerEvent) => {
      lastEv = mv;
      if (!base) return;
      const dx = Math.round((mv.screenX - startX) * factor);
      const dy = Math.round((mv.screenY - startY) * factor);
      void win.setPosition(new PhysicalPosition(base.x + dx, base.y + dy));
    };

    const onUp = () => {
      el.removeEventListener('pointermove', onMove);
      el.removeEventListener('pointerup', onUp);
    };

    el.addEventListener('pointermove', onMove);
    el.addEventListener('pointerup', onUp);

    base = await win.outerPosition();
    if (lastEv) onMove(lastEv);
  }, []);

  return (
    <div
      className="h-8 flex-shrink-0 select-none"
      onPointerDown={onPointerDown}
    />
  );
}
