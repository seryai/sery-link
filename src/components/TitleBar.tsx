import { getCurrentWindow } from '@tauri-apps/api/window';

export function TitleBar() {
  return (
    <div
      data-tauri-drag-region
      className="h-8 flex-shrink-0 select-none"
      onMouseDown={(e) => {
        if (e.buttons === 1) {
          getCurrentWindow().startDragging();
        }
      }}
    />
  );
}
