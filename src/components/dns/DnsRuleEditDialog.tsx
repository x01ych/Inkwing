import { Plus, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  DNS_ACTIONS,
  DNS_MATCHER_GROUPS,
  KNOWN_DNS_MATCHERS,
  type DnsMatcher,
  type DnsRuleInput,
  type DnsRuleView,
} from '../../api/dns';
import type { Scope } from '../../api/rules';
import { Alert, AlertDescription, AlertTitle } from '../ui/alert';
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
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '../ui/select';
import { Switch } from '../ui/switch';

interface Props {
  open: boolean;
  initial: DnsRuleView | null;
  /** DNS server tags from the merged servers list — used for the
   * "server" picker. */
  serverOptions: string[];
  showScopePicker?: boolean;
  initialScope?: Scope;
  onCancel: () => void;
  onSubmit: (input: DnsRuleInput, scope: Scope) => Promise<void>;
}

export default function DnsRuleEditDialog({
  open,
  initial,
  serverOptions,
  showScopePicker = false,
  initialScope = 'per_config',
  onCancel,
  onSubmit,
}: Props) {
  const [matchers, setMatchers] = useState<DnsMatcher[]>([]);
  const [server, setServer] = useState<string | null>(null);
  const [action, setAction] = useState<string | null>(null);
  const [invert, setInvert] = useState(false);
  const [scope, setScope] = useState<Scope>(initialScope);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState('');

  useEffect(() => {
    if (!open) return;
    if (initial) {
      setMatchers(initial.matchers.map((m) => ({ ...m })));
      setServer(initial.server);
      setAction(initial.action);
      setInvert(initial.invert);
    } else {
      setMatchers([{ kind: 'domain_suffix', values: [] }]);
      setServer(serverOptions[0] ?? null);
      setAction(null);
      setInvert(false);
      setScope(initialScope);
    }
    setErr('');
  }, [open, initial, serverOptions, initialScope]);

  function updateMatcher(idx: number, patch: Partial<DnsMatcher>) {
    setMatchers((prev) => prev.map((m, i) => (i === idx ? { ...m, ...patch } : m)));
  }
  function addMatcher() {
    setMatchers((prev) => [...prev, { kind: 'domain', values: [] }]);
  }
  function removeMatcher(idx: number) {
    setMatchers((prev) => prev.filter((_, i) => i !== idx));
  }

  async function handleOk() {
    setErr('');
    if (matchers.length === 0) {
      setErr('At least one matcher is required.');
      return;
    }
    if (matchers.some((m) => m.values.length === 0)) {
      setErr('Each matcher needs at least one value.');
      return;
    }
    if ((!action || action === 'route') && !server) {
      setErr('Pick a DNS server (or set a non-route action).');
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(
        {
          matchers,
          server: action && action !== 'route' ? null : server,
          action: action ?? null,
          invert,
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
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>{initial ? 'Edit DNS rule' : 'New DNS rule'}</DialogTitle>
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
            <Label>
              Matchers
              <span className="ml-2 text-xs font-normal text-muted-foreground">
                multi-value within = OR; across = AND
              </span>
            </Label>
            <div className="space-y-2">
              {matchers.map((m, idx) => (
                <div key={idx} className="flex items-start gap-2">
                  <Select
                    value={m.kind}
                    onValueChange={(v) => updateMatcher(idx, { kind: v })}
                  >
                    <SelectTrigger className="w-56">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {DNS_MATCHER_GROUPS.map((g) => (
                        <SelectGroup key={g.label}>
                          <SelectLabel>{g.label}</SelectLabel>
                          {g.kinds.map((k) => (
                            <SelectItem key={k} value={k}>
                              {k}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      ))}
                    </SelectContent>
                  </Select>
                  <Input
                    value={m.values.join(', ')}
                    placeholder="comma-separated values"
                    onChange={(e) =>
                      updateMatcher(idx, {
                        values: e.target.value
                          .split(',')
                          .map((s) => s.trim())
                          .filter(Boolean),
                      })
                    }
                    className="flex-1"
                  />
                  <Button
                    variant="outline"
                    size="icon"
                    onClick={() => removeMatcher(idx)}
                    disabled={matchers.length === 1}
                  >
                    <Trash2 className="h-4 w-4 text-destructive" />
                  </Button>
                </div>
              ))}
              <Button variant="outline" size="sm" className="w-full" onClick={addMatcher}>
                <Plus className="h-4 w-4" />
                Add matcher
              </Button>
              {matchers.some((m) => !KNOWN_DNS_MATCHERS.includes(m.kind as never)) && (
                <Alert>
                  <AlertTitle>Unknown matcher kind</AlertTitle>
                  <AlertDescription>sing-box may reject it.</AlertDescription>
                </Alert>
              )}
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label>Action</Label>
              <Select
                value={action ?? '__none__'}
                onValueChange={(v) => setAction(v === '__none__' ? null : v)}
              >
                <SelectTrigger>
                  <SelectValue placeholder="(default: route)" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">(default: route)</SelectItem>
                  {DNS_ACTIONS.map((a) => (
                    <SelectItem key={a} value={a}>
                      {a}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>
                DNS server tag
                {(!action || action === 'route') && <span className="text-destructive"> *</span>}
              </Label>
              <Select
                value={server ?? '__none__'}
                disabled={action != null && action !== 'route'}
                onValueChange={(v) => setServer(v === '__none__' ? null : v)}
              >
                <SelectTrigger>
                  <SelectValue placeholder="select server" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">(none)</SelectItem>
                  {serverOptions.map((t) => (
                    <SelectItem key={t} value={t}>
                      {t}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <Switch checked={invert} onCheckedChange={setInvert} id="dns-invert" />
            <Label htmlFor="dns-invert" className="cursor-pointer">
              Invert match
            </Label>
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
          <Button onClick={handleOk} disabled={submitting}>
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
