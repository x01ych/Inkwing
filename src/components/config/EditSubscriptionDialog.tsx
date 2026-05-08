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

interface Props {
  open: boolean;
  initial: Subscription | null;
  onCancel: () => void;
  onSaved: () => void;
}

/** Edit an existing subscription source's name / URL / interval.
 * Doesn't refetch — caller can chain a Refresh after if they want the
 * library entry updated too. */
export default function EditSubscriptionDialog({
  open,
  initial,
  onCancel,
  onSaved,
}: Props) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [intervalHours, setIntervalHours] = useState(0);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState('');

  useEffect(() => {
    if (open && initial) {
      setName(initial.name);
      setUrl(initial.url);
      setIntervalHours(initial.interval_hours);
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
    setBusy(true);
    try {
      await subsApi.update(initial.id, {
        name: name.trim(),
        url: url.trim(),
        interval_hours: intervalHours || 0,
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
            <Label htmlFor="es-interval">Auto-refresh interval (hours, 0 = manual only)</Label>
            <Input
              id="es-interval"
              type="number"
              min={0}
              max={168}
              className="w-40"
              value={intervalHours}
              onChange={(e) => setIntervalHours(parseInt(e.target.value, 10) || 0)}
            />
          </div>

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
