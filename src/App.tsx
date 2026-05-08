import { listen } from '@tauri-apps/api/event';
import i18n from 'i18next';
import { useEffect } from 'react';
import { toast } from 'sonner';
import AppRoutes from './routes';
import { configApi } from './api/config';
import { coreApi, detectPlatform } from './api/core';
import { settingsApi } from './api/settings';
import { useConfigStore } from './store/configStore';
import { useConnectionsStore } from './store/connectionsStore';
import { useCoreStore } from './store/coreStore';
import { useLogsStore } from './store/logsStore';
import { useSettingsStore } from './store/settingsStore';
import { useTunDialogStore } from './store/tunDialogStore';
import { TunPrivilegeDialog } from './components/dialogs/TunPrivilegeDialog';
import { Toaster } from './components/ui/sonner';

export default function App() {
  const setSettings = useSettingsStore((s) => s.setSettings);
  const settings = useSettingsStore((s) => s.settings);
  const setSummary = useConfigStore((s) => s.setSummary);
  const setStatus = useCoreStore((s) => s.setStatus);
  const setLogsEpoch = useLogsStore((s) => s.setEpoch);
  const setConnsEpoch = useConnectionsStore((s) => s.setEpoch);

  // Apply settings.theme + settings.theme_color to <html>. globals.css
  // defines tokens for both .dark and :root (light); the `theme-*`
  // classes are accent overlays that just shift --primary / --ring on
  // top of the dark-mode neutrals.
  useEffect(() => {
    if (!settings) return;
    const html = document.documentElement;
    if (settings.theme === 'light') html.classList.remove('dark');
    else html.classList.add('dark');

    // Drop any previous theme-* class, add the current one.
    Array.from(html.classList)
      .filter((c) => c.startsWith('theme-'))
      .forEach((c) => html.classList.remove(c));
    html.classList.add(`theme-${settings.theme_color || 'zinc'}`);
  }, [settings?.theme, settings?.theme_color]);

  // Sync language to i18next so all useTranslation() consumers re-render.
  useEffect(() => {
    if (!settings?.language) return;
    if (i18n.language !== settings.language) {
      i18n.changeLanguage(settings.language).catch(() => {});
    }
  }, [settings?.language]);

  // Boot wiring:
  // 1. Pre-fetch settings + the active ConfigSummary so Rules /
  //    Dashboard / Proxies render their first frame with real data.
  // 2. Subscribe to `library:changed` so any later select / add /
  //    remove / refresh keeps the summary fresh (without this Rules
  //    keeps the previous config's outbound dropdown).
  // 3. **Auto-start sing-box if there's an active config and core is
  //    stopped.** This is the boot equivalent of "select = use" — when
  //    the user re-opens the app and an active config is already
  //    persisted, they expect the proxy to come up automatically. The
  //    backend's hydrate_on_startup only loads the cache; spawning
  //    sing-box from there bumps into tauri::State<'_> lifetime
  //    constraints, so we drive the autostart from here on the
  //    frontend.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    settingsApi.get().then(setSettings).catch(() => {});
    // Prime the per-store epoch from the current core status. After this,
    // updates flow via core:state events (handled below) — this initial
    // fetch is for the case where the app boots into an already-running
    // core (e.g. autostart on a previous window).
    coreApi
      .status()
      .then((st) => {
        setStatus(st);
        setLogsEpoch(st.epoch);
        setConnsEpoch(st.epoch);
      })
      .catch(() => {});

    const refreshSummary = async () => {
      try {
        const s = await configApi.activeSummary();
        if (!cancelled) setSummary(s);
        return s;
      } catch {
        return null;
      }
    };

    (async () => {
      const summary = await refreshSummary();
      if (cancelled || !summary) return;

      // Self-heal: if persisted settings claim tun_enabled=true but the
      // platform isn't capable (Linux: no CAP_NET_ADMIN; Windows: not
      // elevated), starting sing-box would crash it on TUN-bind. Reset
      // the flag to false and pop the privilege dialog so the user can
      // retry the elevation. macOS is excepted — its privilege check
      // always reports false even when authorisation will succeed at
      // spawn time, so we don't want to second-guess it on boot.
      const platform = detectPlatform();
      try {
        const [s, priv] = await Promise.all([
          settingsApi.get(),
          coreApi.checkPrivilege(),
        ]);
        if (
          s.tun_enabled &&
          !priv.tun_capable &&
          (platform === 'linux' || platform === 'windows')
        ) {
          // Roll back persisted tun_enabled so subsequent core_start
          // calls don't keep injecting a TUN inbound that can't bind.
          const next = await settingsApi.set({ tun_enabled: false });
          if (!cancelled) setSettings(next);
          useTunDialogStore.getState().show({ platform, hint: priv.hint });
          return;
        }
      } catch {
        /* fall through to autostart on any error */
      }

      // Autostart: if sing-box isn't already up, start it. Surface
      // failures via toast (silent failure here means the user opens
      // the app and wonders why nothing's running) and update the
      // store directly so the Dashboard reflects the new state without
      // waiting for the core:state event to round-trip.
      try {
        const cur = await coreApi.status();
        if (cancelled || cur.running) return;
        const st = await coreApi.start();
        if (cancelled) return;
        setStatus(st);
        setLogsEpoch(st.epoch);
        setConnsEpoch(st.epoch);
      } catch (e) {
        const msg = String((e as Error)?.message ?? e);
        console.warn('[boot] autostart failed:', e);
        toast.error(`Autostart failed: ${msg}`, { duration: 8000 });
      }
    })();

    listen('library:changed', () => {
      refreshSummary();
    })
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch(() => {});

    // On every core:state transition, refetch status (CoreStatus carries
    // the current epoch) and propagate the new epoch into the per-stream
    // stores so they drop stale events from the previous session.
    let unlistenCoreState: (() => void) | null = null;
    listen('core:state', () => {
      coreApi
        .status()
        .then((st) => {
          setStatus(st);
          setLogsEpoch(st.epoch);
          setConnsEpoch(st.epoch);
        })
        .catch(() => {});
    })
      .then((un) => {
        if (cancelled) un();
        else unlistenCoreState = un;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
      unlistenCoreState?.();
    };
  }, [setSettings, setSummary, setStatus, setLogsEpoch, setConnsEpoch]);

  return (
    <>
      <AppRoutes />
      <TunPrivilegeDialog />
      <Toaster richColors position="top-right" />
    </>
  );
}
