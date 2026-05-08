import {
  Activity,
  ChevronLeft,
  ChevronRight,
  FileText,
  GitBranch,
  Globe,
  Globe2,
  LayoutDashboard,
  ListOrdered,
  Network,
  Rocket,
  Settings as SettingsIcon,
  Share2,
  ShieldCheck,
} from 'lucide-react';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink, Outlet, useLocation } from 'react-router-dom';
import { toast } from 'sonner';
import { coreApi, detectPlatform, isPrivilegeRequiredError } from '../../api/core';
import { settingsApi, type ProxyMode } from '../../api/settings';
import { useSettingsStore } from '../../store/settingsStore';
import { useTunDialogStore } from '../../store/tunDialogStore';
import { Switch } from '../ui/switch';
import { ToggleGroup, ToggleGroupItem } from '../ui/toggle-group';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../ui/tooltip';
import WindowControls, { HeaderMacInset } from './WindowControls';
import { cn } from '../../lib/utils';

interface NavItem {
  to: string;
  /** i18n key under nav.* */
  i18nKey: string;
  icon: React.ReactNode;
}

const ITEMS: NavItem[] = [
  { to: '/', i18nKey: 'dashboard', icon: <LayoutDashboard size={16} /> },
  { to: '/config', i18nKey: 'config', icon: <FileText size={16} /> },
  { to: '/proxies', i18nKey: 'proxies', icon: <Network size={16} /> },
  { to: '/route', i18nKey: 'route', icon: <ListOrdered size={16} /> },
  { to: '/dns', i18nKey: 'dns', icon: <Globe2 size={16} /> },
  { to: '/logs', i18nKey: 'logs', icon: <Activity size={16} /> },
  { to: '/connections', i18nKey: 'connections', icon: <Share2 size={16} /> },
  { to: '/settings', i18nKey: 'settings', icon: <SettingsIcon size={16} /> },
];

const MODE_ICON: Record<ProxyMode, React.ReactNode> = {
  rule: <GitBranch size={14} />,
  global: <Globe size={14} />,
  direct: <Rocket size={14} />,
};
const MODE_NEXT: Record<ProxyMode, ProxyMode> = {
  rule: 'global',
  global: 'direct',
  direct: 'rule',
};

export default function AppShell() {
  const location = useLocation();
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState(false);

  const activeKey = useMemo(() => {
    const match = ITEMS.map((i) => i.to)
      .filter((p) => location.pathname === p || location.pathname.startsWith(p + '/'))
      .sort((a, b) => b.length - a.length)[0];
    return ITEMS.find((i) => i.to === match)?.i18nKey ?? '';
  }, [location.pathname]);

  return (
    <TooltipProvider delayDuration={300}>
      <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
        <div className="flex flex-1 overflow-hidden">
        <aside
          className={cn(
            'flex flex-col border-r border-border bg-sidebar transition-[width] duration-200',
            collapsed ? 'w-14' : 'w-52'
          )}
        >
          <div
            className={cn(
              'flex h-14 items-center border-b border-border px-4 font-semibold',
              collapsed && 'justify-center px-0'
            )}
          >
            {collapsed ? 'IW' : 'Inkwing'}
          </div>

          {/* Mode strip — top of sidebar, directly under the brand. */}
          <ModeStrip collapsed={collapsed} />

          <nav className="flex-1 space-y-1 overflow-y-auto px-2 py-3">
            {ITEMS.map((it) => (
              <NavLink
                key={it.to}
                to={it.to}
                end={it.to === '/'}
                className={({ isActive }) =>
                  cn(
                    // border-l-2 with a transparent default so the row width
                    // doesn't jump when the active accent bar appears.
                    'flex items-center gap-2 rounded-md border-l-2 border-transparent px-2 py-2 text-sm transition-colors',
                    'text-sidebar-foreground/70 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
                    isActive &&
                      'border-foreground bg-sidebar-accent text-sidebar-accent-foreground font-semibold'
                  )
                }
              >
                <span className="flex h-5 w-5 shrink-0 items-center justify-center">{it.icon}</span>
                {!collapsed && <span className="truncate">{t(`nav.${it.i18nKey}`)}</span>}
              </NavLink>
            ))}
          </nav>

          <TunStrip collapsed={collapsed} />

          <button
            onClick={() => setCollapsed((c) => !c)}
            className="flex h-9 items-center justify-center border-t border-border text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
            title={collapsed ? 'Expand' : 'Collapse'}
          >
            {collapsed ? <ChevronRight size={14} /> : <ChevronLeft size={14} />}
          </button>
        </aside>

        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Header doubles as the window-drag region now that the
            * dedicated title bar is gone. Left inset reserves space for
            * macOS traffic lights (titleBarStyle: "Overlay" overlays them
            * here). Right side hosts the Win/Linux min/max/close. The
            * title text sits in the middle and is also draggable. */}
          <header
            data-tauri-drag-region
            className="flex h-10 shrink-0 items-center border-b border-border bg-sidebar text-sm font-medium text-muted-foreground select-none"
          >
            <HeaderMacInset />
            <span data-tauri-drag-region className="flex-1 px-6">
              {activeKey ? t(`nav.${activeKey}`) : ''}
            </span>
            <WindowControls />
          </header>
          <main className="flex-1 overflow-auto p-6">
            <Outlet />
          </main>
        </div>
        </div>
      </div>
    </TooltipProvider>
  );
}

/** Top-of-sidebar Mode strip (rule / global / direct). Collapsed = single
 * icon button that cycles through modes. Expanded = three-cell text
 * ToggleGroup. Mode-change posts to settings_set; backend auto-restarts
 * sing-box if the value actually changed. */
function ModeStrip({ collapsed }: { collapsed: boolean }) {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [busy, setBusy] = useState(false);
  const { t } = useTranslation();

  async function setMode(mode: ProxyMode) {
    if (!settings || mode === settings.proxy_mode) return;
    setBusy(true);
    try {
      const s = await settingsApi.set({ proxy_mode: mode });
      setSettings(s);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  const mode = (settings?.proxy_mode ?? 'rule') as ProxyMode;

  // High-contrast active style for the three Mode cells. We override
  // the default `data-[state=on]:bg-accent` (which collapses to the
  // same flat dark grey as hover/muted in zinc dark mode) with a
  // primary-tinted background so the picked mode is unmistakable.
  const modeCellActive =
    'data-[state=on]:bg-primary data-[state=on]:text-primary-foreground data-[state=on]:font-semibold data-[state=on]:shadow-sm';

  if (collapsed) {
    return (
      <div className="flex justify-center border-b border-border py-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              onClick={() => setMode(MODE_NEXT[mode])}
              disabled={!settings || busy}
              // Always-bright icon: there's only one button in the
              // collapsed strip and it represents the *current* mode,
              // so the icon should be at full foreground colour
              // (otherwise the dim grey looks like a disabled control).
              className={cn(
                'flex h-8 w-8 items-center justify-center rounded-md text-foreground hover:bg-sidebar-accent',
                busy && 'opacity-50'
              )}
            >
              {MODE_ICON[mode]}
            </button>
          </TooltipTrigger>
          <TooltipContent side="right">
            {t('sidebar.mode_tooltip', { mode: t(`sidebar.mode_${mode}`) })}
          </TooltipContent>
        </Tooltip>
      </div>
    );
  }

  return (
    <div className="border-b border-border px-3 py-3">
      <div className="mb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
        {t('sidebar.mode')}
      </div>
      <ToggleGroup
        type="single"
        value={mode}
        disabled={!settings || busy}
        onValueChange={(v) => v && setMode(v as ProxyMode)}
        className="grid grid-cols-3"
        variant="outline"
        size="sm"
      >
        <ToggleGroupItem value="rule" aria-label="Rule mode" className={modeCellActive}>
          {t('sidebar.mode_rule')}
        </ToggleGroupItem>
        <ToggleGroupItem value="global" aria-label="Global mode" className={modeCellActive}>
          {t('sidebar.mode_global')}
        </ToggleGroupItem>
        <ToggleGroupItem value="direct" aria-label="Direct mode" className={modeCellActive}>
          {t('sidebar.mode_direct')}
        </ToggleGroupItem>
      </ToggleGroup>
    </div>
  );
}

/** Bottom-of-sidebar TUN switch. Same backend semantics as Mode —
 * settings_set + auto-restart. */
function TunStrip({ collapsed }: { collapsed: boolean }) {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [busy, setBusy] = useState(false);
  const { t } = useTranslation();

  async function toggle(on: boolean) {
    if (!settings) return;
    setBusy(true);
    try {
      // Pre-flight on toggle ON: check whether the current process can
      // run sing-box in TUN mode. macOS always reports !tun_capable
      // (per-session osascript admin) and we want the dialog every
      // time TUN goes from OFF→ON; Linux/Windows only need the dialog
      // when capability is genuinely missing. If the dialog opens we
      // bail out early — settings_set is only called after the user
      // grants the privilege.
      if (on) {
        const platform = detectPlatform();
        const priv = await coreApi.checkPrivilege();
        if (!priv.tun_capable) {
          useTunDialogStore.getState().show({ platform, hint: priv.hint });
          return;
        }
      }
      const s = await settingsApi.set({ tun_enabled: on });
      setSettings(s);
    } catch (e) {
      // Backend safety net: settings_set may also reject with a
      // structured PrivilegeRequired error if the capability slipped
      // between the pre-flight and the actual flip. Funnel both into
      // the dialog instead of a useless toast.
      if (isPrivilegeRequiredError(e)) {
        useTunDialogStore.getState().show({
          platform: (e.platform as ReturnType<typeof detectPlatform>) || 'other',
          hint: e.hint,
        });
      } else {
        toast.error(String((e as Error)?.message ?? e));
      }
    } finally {
      setBusy(false);
    }
  }

  const tunOn = !!settings?.tun_enabled;

  if (collapsed) {
    return (
      <div className="flex justify-center border-t border-border py-2">
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              onClick={() => toggle(!tunOn)}
              disabled={!settings || busy}
              className={cn(
                'flex h-8 w-8 items-center justify-center rounded-md hover:bg-sidebar-accent',
                tunOn ? 'text-foreground' : 'text-muted-foreground',
                busy && 'opacity-50'
              )}
            >
              <ShieldCheck size={20} strokeWidth={2.25} />
            </button>
          </TooltipTrigger>
          <TooltipContent side="right">
            {t('sidebar.tun_tooltip', { state: tunOn ? 'on' : 'off' })}
          </TooltipContent>
        </Tooltip>
      </div>
    );
  }

  return (
    <div className="flex items-center justify-between border-t border-border px-3 py-3">
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex items-center gap-2 text-sm text-foreground">
            <ShieldCheck
              size={20}
              strokeWidth={2.25}
              className={tunOn ? 'text-foreground' : 'text-muted-foreground'}
            />
            <span className="font-medium">{t('sidebar.tun')}</span>
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{t('sidebar.tun_help')}</TooltipContent>
      </Tooltip>
      <Switch
        checked={tunOn}
        disabled={!settings || busy}
        onCheckedChange={toggle}
      />
    </div>
  );
}
