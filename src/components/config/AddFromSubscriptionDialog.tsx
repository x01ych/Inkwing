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

interface Props {
  open: boolean;
  onCancel: () => void;
  /** Called after a successful add+fetch, with the new ConfigEntry id. */
  onAdded: (newConfigId: string) => void;
}

export default function AddFromSubscriptionDialog({ open, onCancel, onAdded }: Props) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [intervalHours, setIntervalHours] = useState(24);
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
    setIntervalHours(24);
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
    setBusy(true);
    try {
      const sub = await subsApi.add({
        name: name.trim(),
        url: url.trim(),
        interval_hours: intervalHours || 0,
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
            <Label htmlFor="sub-interval">Auto-refresh interval (hours, 0 = manual only)</Label>
            <Input
              id="sub-interval"
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
            Add & fetch
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
