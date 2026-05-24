import { useEffect, useState } from 'react';
import { subsApi, type Subscription } from '../../api/subscriptions';
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
  initial: Subscription | null;
  onCancel: () => void;
  onSaved: () => void;
}

type ScheduleMode = 'manual' | 'every' | 'daily';

function deriveMode(s: Subscription | null): ScheduleMode {
  if (!s) return 'manual';
  if (s.daily_update_at) return 'daily';
  if (s.interval_hours > 0) return 'every';
  return 'manual';
}

/** Edit an existing subscription source's name / URL / schedule.
 * Saving doesn't refetch — caller can chain a Refresh after if they want
 * the existing library entry updated too. */
export default function EditSubscriptionDialog({
  open,
  initial,
  onCancel,
  onSaved,
}: Props) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [mode, setMode] = useState<ScheduleMode>('manual');
  const [intervalHours, setIntervalHours] = useState(0);
  const [dailyAt, setDailyAt] = useState('03:00');
  const [keepLastN, setKeepLastN] = useState(5);
  const [autoSwitch, setAutoSwitch] = useState(true);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');

  useEffect(() => {
    if (open && initial) {
      setName(initial.name);
      setUrl(initial.url);
      setMode(deriveMode(initial));
      setIntervalHours(initial.interval_hours || 0);
      setDailyAt(initial.daily_update_at || '03:00');
      setKeepLastN(initial.keep_last_n ?? 5);
      setAutoSwitch(initial.auto_switch_to_new ?? true);
      setErr('');
    }
  }, [open, initial]);

  async function handleOk() {
    if (!initial) return;
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
      await subsApi.update(initial.id, {
        name: name.trim(),
        url: url.trim(),
        interval_hours: mode === 'every' ? intervalHours : 0,
        daily_update_at: mode === 'daily' ? dailyAt : null,
        keep_last_n: keepLastN,
        auto_switch_to_new: autoSwitch,
      });
      onSaved();
    } catch (e) {
      setErr(String((e as Error)?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onCancel()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Edit subscription "{initial?.name ?? ''}"</DialogTitle>
          <DialogDescription>
            Saving doesn't re-fetch — use the card's Refresh button afterwards if you want the
            existing library entry updated.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <div className="space-y-2">
            <Label htmlFor="es-name">
              Name<span className="text-destructive"> *</span>
            </Label>
            <Input
              id="es-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="es-url">
              URL<span className="text-destructive"> *</span>
            </Label>
            <Input
              id="es-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="es-mode">Update schedule</Label>
            <Select value={mode} onValueChange={(v) => setMode(v as ScheduleMode)}>
              <SelectTrigger id="es-mode" className="w-full">
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
              <Label htmlFor="es-interval">Interval (hours, 1-168)</Label>
              <Input
                id="es-interval"
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
              <Label htmlFor="es-daily">Local time (HH:MM, 24-hour)</Label>
              <Input
                id="es-daily"
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
                <Label htmlFor="es-keep">Keep last N library entries</Label>
                <Input
                  id="es-keep"
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
                  id="es-auto-switch"
                  checked={autoSwitch}
                  onCheckedChange={setAutoSwitch}
                />
                <div className="flex-1 space-y-1">
                  <Label htmlFor="es-auto-switch" className="cursor-pointer">
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
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
