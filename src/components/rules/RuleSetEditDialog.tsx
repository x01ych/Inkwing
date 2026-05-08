import { useEffect, useState } from 'react';
import type { RuleSetInput, RuleSetView, Scope } from '../../api/rules';
import { Alert, AlertDescription } from '../ui/alert';
import { Button } from '../ui/button';
import {
  Dialog,
  DialogContent,
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

interface Props {
  open: boolean;
  initial: RuleSetView | null;
  outboundOptions: string[];
  showScopePicker?: boolean;
  initialScope?: Scope;
  onCancel: () => void;
  onSubmit: (input: RuleSetInput, scope: Scope) => Promise<void>;
}

export default function RuleSetEditDialog({
  open,
  initial,
  outboundOptions,
  showScopePicker = false,
  initialScope = 'per_config',
  onCancel,
  onSubmit,
}: Props) {
  const [tag, setTag] = useState('');
  const [kind, setKind] = useState<'local' | 'remote'>('remote');
  const [format, setFormat] = useState<'binary' | 'source'>('binary');
  const [url, setUrl] = useState('');
  const [path, setPath] = useState('');
  const [downloadDetour, setDownloadDetour] = useState<string | null>(null);
  const [updateInterval, setUpdateInterval] = useState('');
  const [scope, setScope] = useState<Scope>(initialScope);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState('');

  useEffect(() => {
    if (!open) return;
    if (initial) {
      setTag(initial.tag);
      setKind((initial.kind as 'local' | 'remote') || 'remote');
      setFormat((initial.format as 'binary' | 'source') || 'binary');
      setUrl(initial.url ?? '');
      setPath(initial.path ?? '');
      setDownloadDetour(initial.download_detour);
      setUpdateInterval(initial.update_interval ?? '');
    } else {
      setTag('');
      setKind('remote');
      setFormat('binary');
      setUrl('');
      setPath('');
      setDownloadDetour(null);
      setUpdateInterval('1d');
      setScope(initialScope);
    }
    setErr('');
  }, [open, initial, initialScope]);

  async function handleOk() {
    setErr('');
    if (!tag.trim()) {
      setErr('Tag is required.');
      return;
    }
    if (kind === 'remote' && !url.trim()) {
      setErr('URL is required for remote rule_set.');
      return;
    }
    if (kind === 'local' && !path.trim()) {
      setErr('Path is required for local rule_set.');
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(
        {
          tag: tag.trim(),
          kind,
          format,
          url: kind === 'remote' ? url.trim() : null,
          path: kind === 'local' ? path.trim() : null,
          download_detour: kind === 'remote' ? downloadDetour : null,
          update_interval: kind === 'remote' ? updateInterval.trim() || null : null,
        },
        scope,
      );
    } catch (e) {
      setErr(String((e as Error)?.message ?? e));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onCancel()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{initial ? `Edit rule_set #${initial.id}` : 'New rule_set'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {showScopePicker && (
            <div className="space-y-2">
              <Label>Scope</Label>
              <Select value={scope} onValueChange={(v) => setScope(v as Scope)}>
                <SelectTrigger className="w-72">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="per_config">Per-config (this config only)</SelectItem>
                  <SelectItem value="global">Global (every config)</SelectItem>
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="space-y-2">
            <Label htmlFor="rs-tag">
              Tag<span className="text-destructive"> *</span>
            </Label>
            <Input
              id="rs-tag"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder="my-ruleset"
            />
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label>Type</Label>
              <Select value={kind} onValueChange={(v) => setKind(v as 'local' | 'remote')}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="remote">Remote (URL)</SelectItem>
                  <SelectItem value="local">Local (file)</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>Format</Label>
              <Select value={format} onValueChange={(v) => setFormat(v as 'binary' | 'source')}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="binary">binary (.srs)</SelectItem>
                  <SelectItem value="source">source (.json)</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          {kind === 'remote' ? (
            <>
              <div className="space-y-2">
                <Label htmlFor="rs-url">
                  URL<span className="text-destructive"> *</span>
                </Label>
                <Input
                  id="rs-url"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="https://example.com/ruleset.srs"
                />
              </div>
              <div className="space-y-2">
                <Label>Download detour</Label>
                <Select
                  value={downloadDetour ?? '__default__'}
                  onValueChange={(v) =>
                    setDownloadDetour(v === '__default__' ? null : v)
                  }
                >
                  <SelectTrigger className="w-80">
                    <SelectValue placeholder="(default)" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__default__">(default)</SelectItem>
                    {outboundOptions.map((t) => (
                      <SelectItem key={t} value={t}>
                        {t}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  Outbound used to fetch the rule_set itself. Pick a working proxy if the URL is
                  geo-restricted; leave blank to use the default route.
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="rs-interval">Update interval</Label>
                <Input
                  id="rs-interval"
                  className="w-40"
                  value={updateInterval}
                  onChange={(e) => setUpdateInterval(e.target.value)}
                  placeholder="1d"
                />
                <p className="text-xs text-muted-foreground">
                  e.g. 1d, 12h, 30m. Empty = no auto-refresh.
                </p>
              </div>
            </>
          ) : (
            <div className="space-y-2">
              <Label htmlFor="rs-path">
                Path<span className="text-destructive"> *</span>
              </Label>
              <Input
                id="rs-path"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/path/to/ruleset.srs"
              />
            </div>
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
          <Button onClick={handleOk} disabled={submitting}>
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
