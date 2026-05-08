import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';

import { coreApi } from '../../api/core';
import { settingsApi } from '../../api/settings';
import { useSettingsStore } from '../../store/settingsStore';
import { useTunDialogStore } from '../../store/tunDialogStore';
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '../ui/alert-dialog';
import { Button } from '../ui/button';

/**
 * Sits in the React tree once at app root. Opens whenever
 * `useTunDialogStore.show(...)` is called — which happens from
 * (1) sidebar TunStrip pre-flight when the user toggles ON without
 * privileges, and (2) App.tsx boot when settings has tun_enabled=true
 * but core_check_privilege says we're not capable.
 *
 * Dispatches the per-platform elevation flow:
 *   - linux   → coreApi.grantTunCapabilityLinux (pkexec setcap)
 *   - windows → coreApi.relaunchAsAdminWindows  (PowerShell -Verb RunAs)
 *   - macos   → coreApi.testMacosAdmin          (osascript probe)
 */
export function TunPrivilegeDialog() {
  const { t } = useTranslation();
  const open = useTunDialogStore((s) => s.open);
  const platform = useTunDialogStore((s) => s.platform);
  const hint = useTunDialogStore((s) => s.hint);
  const hide = useTunDialogStore((s) => s.hide);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [busy, setBusy] = useState(false);

  const bodyKey =
    platform === 'linux'
      ? 'tun_dialog.linux_body'
      : platform === 'windows'
      ? 'tun_dialog.windows_body'
      : platform === 'macos'
      ? 'tun_dialog.macos_body'
      : 'tun_dialog.linux_body';

  const grantLabel =
    platform === 'linux'
      ? t('tun_dialog.grant_linux')
      : platform === 'windows'
      ? t('tun_dialog.grant_windows')
      : platform === 'macos'
      ? t('tun_dialog.grant_macos')
      : t('tun_dialog.grant_other');

  async function onGrant() {
    setBusy(true);
    try {
      if (platform === 'linux') {
        await coreApi.grantTunCapabilityLinux();
        const s = await settingsApi.set({ tun_enabled: true });
        setSettings(s);
        toast.success(t('tun_dialog.grant_success'));
        hide();
      } else if (platform === 'windows') {
        // app.exit(0) on the backend means this promise rejects with a
        // closed-channel error; treat it as success and inform the user.
        toast.message(t('tun_dialog.windows_relaunching'));
        try {
          await coreApi.relaunchAsAdminWindows();
        } catch {
          /* expected — process is exiting */
        }
        // No hide() — the window will be gone in moments.
      } else if (platform === 'macos') {
        await coreApi.testMacosAdmin();
        const s = await settingsApi.set({ tun_enabled: true });
        setSettings(s);
        toast.success(t('tun_dialog.grant_success'));
        hide();
      } else {
        hide();
      }
    } catch (e) {
      const msg = (e as Error)?.message ?? String(e);
      toast.error(t('tun_dialog.grant_failed', { message: msg }));
    } finally {
      setBusy(false);
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(o) => {
        if (!o && !busy) hide();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('tun_dialog.title')}</AlertDialogTitle>
          <AlertDialogDescription className="space-y-2">
            <span className="block">{t(bodyKey)}</span>
            {hint ? (
              <span className="block text-xs text-muted-foreground">
                <span className="font-medium">{t('tun_dialog.hint_label')}:</span> {hint}
              </span>
            ) : null}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy}>{t('tun_dialog.cancel')}</AlertDialogCancel>
          <Button onClick={onGrant} disabled={busy}>
            {busy ? t('tun_dialog.granting') : grantLabel}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
