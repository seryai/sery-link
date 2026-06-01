import { useNavigate, useLocation } from 'react-router-dom';
import { Bell, History, LayoutGrid, Plus, Settings } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { ConnectModal } from './ConnectModal';

export function TitleBar() {
  const navigate = useNavigate();
  const location = useLocation();
  const {
    authenticated,
    connectionStatus,
    schemaNotifications,
    setAddSourceOpen,
    showConnectModal,
    setShowConnectModal,
    connectDefaultKey,
    setConnectDefaultKey,
    stats,
  } = useAgentStore();

  const unread = schemaNotifications.filter((n) => !n.read).length;
  const isActive = (path: string) => location.pathname.startsWith(path);

  // Status indicator config
  const status = !authenticated
    ? { dot: 'bg-slate-400', label: 'Local only', clickable: true }
    : connectionStatus === 'online'
    ? { dot: 'bg-emerald-500', label: 'Connected', clickable: false }
    : connectionStatus === 'error'
    ? { dot: 'bg-rose-500', label: 'Error', clickable: false }
    : { dot: 'bg-amber-500 animate-pulse', label: 'Connecting…', clickable: false };

  return (
    <>
      <div
        data-tauri-drag-region
        className="h-10 flex-shrink-0 flex items-center border-b border-black/[0.07] dark:border-white/[0.08] bg-slate-50 dark:bg-slate-900/60 px-3"
      >
        {/* Left: pl-[72px] clears macOS traffic lights */}
        <div className="flex items-center gap-2 pl-[72px]" data-tauri-drag-region>
          <button
            onClick={() => status.clickable && setShowConnectModal(true)}
            title={status.label}
            className={`flex items-center gap-1.5 rounded-md p-1.5 transition-colors ${
              status.clickable
                ? 'hover:bg-black/5 dark:hover:bg-white/5 cursor-pointer'
                : 'cursor-default'
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full flex-shrink-0 ${status.dot}`}
            />
            <span className="text-[14px] text-slate-600 dark:text-slate-300">
              {status.label}
            </span>
            {stats && (
              <span className="text-[14px] text-slate-400 dark:text-slate-500">
                · {stats.queries_today}{' '}
                {stats.queries_today === 1 ? 'query' : 'queries'} today
              </span>
            )}
          </button>
        </div>

        {/* Center drag spacer */}
        <div className="flex-1" data-tauri-drag-region />

        {/* Right actions */}
        <div className="flex items-center gap-0.5">
          <TitleBtn
            onClick={() => navigate('/')}
            label="Overview"
            active={location.pathname === '/'}
          >
            <LayoutGrid className="h-4 w-4" />
          </TitleBtn>
          <TitleBtn onClick={() => setAddSourceOpen(true)} label="New source">
            <Plus className="h-4 w-4" />
          </TitleBtn>
          <TitleBtn
            onClick={() => navigate('/history')}
            label="History"
            active={isActive('/history')}
          >
            <History className="h-4 w-4" />
          </TitleBtn>
          <TitleBtn
            onClick={() => navigate('/notifications')}
            label="Notifications"
            active={isActive('/notifications')}
          >
            <div className="relative">
              <Bell className="h-4 w-4" />
              {unread > 0 && (
                <span className="absolute -right-1 -top-1 flex h-3.5 w-3.5 items-center justify-center rounded-full bg-purple-600 text-[9px] font-bold text-white leading-none">
                  {unread > 9 ? '9+' : unread}
                </span>
              )}
            </div>
          </TitleBtn>
          <TitleBtn
            onClick={() => navigate('/settings')}
            label="Settings"
            active={isActive('/settings')}
          >
            <Settings className="h-4 w-4" />
          </TitleBtn>
        </div>
      </div>

      {showConnectModal && (
        <ConnectModal
          onClose={() => {
            setShowConnectModal(false);
            setConnectDefaultKey(null);
          }}
          defaultKey={connectDefaultKey ?? undefined}
        />
      )}
    </>
  );
}

function TitleBtn({
  onClick,
  label,
  active,
  children,
}: {
  onClick: () => void;
  label: string;
  active?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[13px] transition-colors ${
        active
          ? 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
          : 'text-slate-500 hover:bg-black/5 hover:text-slate-700 dark:text-slate-400 dark:hover:bg-white/5 dark:hover:text-slate-200'
      }`}
    >
      {children}
      <span>{label}</span>
    </button>
  );
}
