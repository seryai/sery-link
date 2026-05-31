import { getCurrentWindow } from '@tauri-apps/api/window';
import { PhysicalPosition } from '@tauri-apps/api/dpi';
import { useCallback } from 'react';

export function TitleBar() {
  const onMouseDown = useCallback(async (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();

    const win = getCurrentWindow();
    const startX = e.screenX;
    const startY = e.screenY;
    const factor = window.devicePixelRatio ?? 1;

    // Register listeners synchronously so the webview keeps mouse capture
    // during the outerPosition() IPC round-trip. Without this, when the
    // window is already focused the first mousemove fires before the listener
    // is added, the browser loses capture, and drag silently stops working.
    let base: Awaited<ReturnType<typeof win.outerPosition>> | null = null;
    let lastMv: MouseEvent | null = null;

    const onMove = (mv: MouseEvent) => {
      lastMv = mv;
      if (!base) return;
      const dx = Math.round((mv.screenX - startX) * factor);
      const dy = Math.round((mv.screenY - startY) * factor);
      void win.setPosition(new PhysicalPosition(base.x + dx, base.y + dy));
    };

    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);

    base = await win.outerPosition();
    if (lastMv) onMove(lastMv);
  }, []);

  return (
    <div
      className="h-8 flex-shrink-0 select-none"
      onMouseDown={onMouseDown}
    />
  );
}
