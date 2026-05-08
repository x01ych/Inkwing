import { Loader2 } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';
import { settingsApi, type Settings } from '../../api/settings';
import { useSettingsStore } from '../../store/settingsStore';
import { Card, CardContent, CardHeader, CardTitle } from '../ui/card';
import { Input } from '../ui/input';
import { Switch } from '../ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/tooltip';

const PROTOCOLS: { key: 'mixed_port' | 'socks_port' | 'http_port'; label: string; hint: string }[] = [
  {
    key: 'mixed_port',
    label: 'Mixed (SOCKS5 + HTTP)',
    hint: 'Single port serving both SOCKS5 and HTTP CONNECT. Most clients want this.',
  },
  {
    key: 'socks_port',
    label: 'SOCKS5 only',
    hint: 'Use only if you need a SOCKS-only port distinct from the mixed one.',
  },
  {
    key: 'http_port',
    label: 'HTTP only',
    hint: 'Use only if you need an HTTP-only proxy port distinct from the mixed one.',
  },
];

/** Manage the runtime mixed/socks/http inbound ports. The settings command
 * auto-restarts sing-box when these change, so the new ports are live
 * within a couple of seconds. */
export default function LocalPortsCard() {
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const [busy, setBusy] = useState<string>('');

  if (!settings) return null;

  async function patch(p: Partial<Settings>) {
    setBusy(Object.keys(p)[0] || '');
    try {
      const s = await settingsApi.set(p);
      setSettings(s);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setBusy('');
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Local proxy ports</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {PROTOCOLS.map(({ key, label, hint }) => {
          const v = settings[key] as number | null;
          const enabled = v != null;
          const isBusy = busy === key;
          return (
            <div key={key} className="flex items-center justify-between gap-3">
              <Tooltip>
                <TooltipTrigger asChild>
                  <span className="text-sm font-medium">{label}</span>
                </TooltipTrigger>
                <TooltipContent>{hint}</TooltipContent>
              </Tooltip>
              <div className="flex items-center gap-2">
                {isBusy && <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />}
                <Switch
                  checked={enabled}
                  disabled={isBusy}
                  onCheckedChange={(checked) => {
                    if (checked) {
                      const defaults: Record<string, number> = {
                        mixed_port: 7890,
                        socks_port: 1080,
                        http_port: 7891,
                      };
                      patch({ [key]: defaults[key] } as Partial<Settings>);
                    } else {
                      patch({ [key]: null } as unknown as Partial<Settings>);
                    }
                  }}
                />
                <Input
                  type="number"
                  min={1}
                  max={65535}
                  disabled={!enabled || isBusy}
                  defaultValue={v ?? ''}
                  className="h-8 w-24"
                  onBlur={(e) => {
                    const n = parseInt(e.target.value, 10);
                    if (!Number.isNaN(n) && n !== v) {
                      patch({ [key]: n } as Partial<Settings>);
                    }
                  }}
                  key={`${key}:${v ?? 'off'}`} /* re-mount on toggle so defaultValue refreshes */
                />
              </div>
            </div>
          );
        })}
        <p className="text-xs text-muted-foreground">
          All ports listen on 127.0.0.1. Changes auto-restart sing-box. Your config file is
          never modified — these inbounds are injected at runtime only.
        </p>
      </CardContent>
    </Card>
  );
}
