import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { settingsApi, type Settings, type ThemeColor } from '../api/settings';
import { useSettingsStore } from '../store/settingsStore';
import LocalPortsCard from '../components/config/LocalPortsCard';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select';
import { Switch } from '../components/ui/switch';

export default function SettingsPage() {
  // Reads from the store hydrated by App.tsx — no per-mount fetch, no
  // spinner flash on tab switch.
  const settings = useSettingsStore((s) => s.settings);
  const setSettings = useSettingsStore((s) => s.setSettings);
  const { t } = useTranslation();

  async function patch(p: Partial<Settings>) {
    try {
      const s = await settingsApi.set(p);
      setSettings(s);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }

  if (!settings) {
    return (
      <div className="flex h-32 items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-6">
      <h2 className="text-2xl font-semibold tracking-tight">{t('settings.title')}</h2>

      <LocalPortsCard />

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.behaviour')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-5">
          <SwitchRow
            label={t('settings.minimize_to_tray')}
            description={t('settings.minimize_to_tray_desc')}
            checked={settings.minimize_to_tray}
            onCheckedChange={(v) => patch({ minimize_to_tray: v })}
          />
          <SwitchRow
            label={t('settings.autostart')}
            description={t('settings.autostart_desc')}
            checked={settings.autostart}
            onCheckedChange={(v) => patch({ autostart: v })}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.latency')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <Label htmlFor="latency-test-url">{t('settings.test_url')}</Label>
          <Input
            id="latency-test-url"
            defaultValue={settings.latency_test_url}
            placeholder="https://www.gstatic.com/generate_204"
            onBlur={(e) =>
              e.target.value !== settings.latency_test_url &&
              patch({ latency_test_url: e.target.value })
            }
          />
          <p className="text-xs text-muted-foreground">{t('settings.test_url_desc')}</p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t('settings.appearance')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-5">
          <div className="space-y-2">
            <Label>{t('settings.theme')}</Label>
            <Select value={settings.theme} onValueChange={(v) => patch({ theme: v })}>
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="dark">{t('settings.theme_dark')}</SelectItem>
                <SelectItem value="light">{t('settings.theme_light')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>{t('settings.accent')}</Label>
            <Select
              value={settings.theme_color || 'zinc'}
              onValueChange={(v) => patch({ theme_color: v as ThemeColor })}
            >
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="zinc">Zinc</SelectItem>
                <SelectItem value="slate">Slate</SelectItem>
                <SelectItem value="blue">Blue</SelectItem>
                <SelectItem value="green">Green</SelectItem>
                <SelectItem value="rose">Rose</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t('settings.accent_desc')}</p>
          </div>
          <div className="space-y-2">
            <Label>{t('settings.language')}</Label>
            <Select value={settings.language} onValueChange={(v) => patch({ language: v })}>
              <SelectTrigger className="w-48">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="en">English</SelectItem>
                <SelectItem value="zh">简体中文</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t('settings.language_note')}</p>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function SwitchRow({
  label,
  description,
  checked,
  onCheckedChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="space-y-1">
        <Label className="text-sm font-medium">{label}</Label>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      <Switch checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}
