import { useState } from 'react';
import { subsApi } from '../../api/subscriptions';
import { Alert, AlertDescription } from '../ui/alert';
import { Button } from '../ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select';
import { Switch } from '../ui/switch';

interface Props {
  open: boolean;
  onCancel: () => void;
  /** Called after a successful add+fetch, with the new ConfigEntry id. */
  onAdded: (newConfigId: string) => void;
}

type ScheduleMode = 'manual' | 'every' | 'daily';

export default function AddFromSubscriptionDialog({ open, onCancel, onAdded }: Props) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [mode, setMode] = useState<ScheduleMode>('every');
  const [intervalHours, setIntervalHours] = useState(24);
  const [dailyAt, setDailyAt] = useState('03:00');
  const [keepLastN, setKeepLastN] = useState(5);
  const [autoSwitch, setAutoSwitch] = useState(true);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');

  // Reset form whenever the dialog re-opens.
  function handleOpenChange(o: boolean) {
    if (!o) {
      onCancel();
      return;
    }
    setName('');
    setUrl('');
    setMode('every');
    setIntervalHours(24);
    setDailyAt('03:00');
    setKeepLastN(5);
    setAutoSwitch(true);
    setErr('');
  }

  async function handleOk() {
    setErr('');
    if (!name.trim()) {
      setErr('Name is required.');
      return;
    }
    if (!url.trim()) {
      setErr('URL is required.');
      return;
    }
    try {
      // Cheap URL validity check.
      new URL(url);
    } catch {
      setErr('Must be a valid URL.');
      return;
    }
    if (mode === 'every' && (intervalHours < 1 || intervalHours > 168)) {
      setErr('Interval must be between 1 and 168 hours.');
      return;
    }
    if (mode === 'daily' && !/^\d{2}:\d{2}$/.test(dailyAt)) {
      setErr('Daily time must be HH:MM (24-hour).');
      return;
    }
    if (keepLastN < 1 || keepLastN > 100) {
      setErr('Keep last N must be between 1 and 100.');
      return;
    }
    setBusy(true);
    try {
      const sub = await subsApi.add({
        name: name.trim(),
        url: url.trim(),
        interval_hours: mode === 'every' ? intervalHours : 0,
        daily_update_at: mode === 'daily' ? dailyAt : null,
        keep_last_n: keepLastN,
        auto_switch_to_new: autoSwitch,
      });
      const newId = await subsApi.apply(sub.id);
      onAdded(newId);
    } catch (e) {
      setErr(String((e as Error)?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Add from subscription URL</DialogTitle>
          <DialogDescription>
            v1 only supports native sing-box JSON subscriptions. The fetched config is saved as
            a NEW entry in your library; your active config isn't replaced.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-2">
            <Label htmlFor="sub-name">
              Name<span className="text-destructive"> *</span>
            </Label>
            <Input
              id="sub-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="My provider"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="sub-url">
              URL<span className="text-destructive"> *</span>
            </Label>
            <Input
              id="sub-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://example.com/sub.json"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="sub-mode">Update schedule</Label>
            <Select value={mode} onValueChange={(v) => setMode(v as ScheduleMode)}>
              <SelectTrigger id="sub-mode" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="manual">Manual only</SelectItem>
                <SelectItem value="every">Every N hours</SelectItem>
                <SelectItem value="daily">Daily at HH:MM</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {mode === 'every' && (
            <div className="space-y-2">
              <Label htmlFor="sub-interval">Interval (hours, 1-168)</Label>
              <Input
                id="sub-interval"
                type="number"
                min={1}
                max={168}
                className="w-40"
                value={intervalHours || ''}
                onChange={(e) => setIntervalHours(parseInt(e.target.value, 10) || 0)}
              />
            </div>
          )}

          {mode === 'daily' && (
            <div className="space-y-2">
              <Label htmlFor="sub-daily">Local time (HH:MM, 24-hour)</Label>
              <Input
                id="sub-daily"
                type="time"
                className="w-40"
                value={dailyAt}
                onChange={(e) => setDailyAt(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                Uses your computer's local time zone. Pick an hour when you're typically
                idle (e.g. 03:00) to avoid the brief disconnect when auto-switch restarts
                sing-box with the new config.
              </p>
            </div>
          )}

          {mode !== 'manual' && (
            <>
              <div className="space-y-2">
                <Label htmlFor="sub-keep">Keep last N library entries</Label>
                <Input
                  id="sub-keep"
                  type="number"
                  min={1}
                  max={100}
                  className="w-40"
                  value={keepLastN}
                  onChange={(e) => setKeepLastN(parseInt(e.target.value, 10) || 1)}
                />
                <p className="text-xs text-muted-foreground">
                  After each auto-update, the oldest entries from this subscription are
                  deleted (except the active one).
                </p>
              </div>

              <div className="flex items-start gap-3">
                <Switch
                  id="sub-auto-switch"
                  checked={autoSwitch}
                  onCheckedChange={setAutoSwitch}
                />
                <div className="flex-1 space-y-1">
                  <Label htmlFor="sub-auto-switch" className="cursor-pointer">
                    Auto-switch active config to the new entry
                  </Label>
                  <p className="text-xs text-muted-foreground">
                    When the current active config is from this subscription, the
                    scheduler will switch to the new entry and restart sing-box (≈2 s
                    disconnect). Disable to only add new entries to the library.
                  </p>
                </div>
              </div>
            </>
          )}

          {err && (
            <Alert variant="destructive">
              <AlertDescription>{err}</AlertDescription>
            </Alert>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button onClick={handleOk} disabled={busy}>
            Add & fetch
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
