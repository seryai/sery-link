export function TitleBar() {
  return (
    <div
      data-tauri-drag-region
      className="h-8 flex-shrink-0 select-none flex items-center justify-center"
    >
      <span
        data-tauri-drag-region
        className="text-[13px] font-medium text-slate-600 dark:text-slate-400 pointer-events-none"
      >
        Sery Link
      </span>
    </div>
  );
}
