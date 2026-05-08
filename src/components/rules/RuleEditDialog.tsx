import { Plus, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import {
  ACTIONS,
  KNOWN_MATCHERS,
  MATCHER_GROUPS,
  type Matcher,
  type RuleInput,
  type RuleView,
  type Scope,
} from '../../api/rules';
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
  initial: RuleView | null;
  outboundOptions: string[];
  /** Only relevant when adding a *new* rule (initial == null). Editing an
   * existing rule keeps its scope; the picker is hidden. */
  showScopePicker?: boolean;
  initialScope?: Scope;
  onCancel: () => void;
  onSubmit: (input: RuleInput, scope: Scope) => Promise<void>;
}

export default function RuleEditDialog({
  open,
  initial,
  outboundOptions,
  showScopePicker = false,
  initialScope = 'per_config',
  onCancel,
  onSubmit,
}: Props) {
  const [matchers, setMatchers] = useState<Matcher[]>([]);
  const [outbound, setOutbound] = useState<string | null>(null);
  const [action, setAction] = useState<string | null>(null);
  const [invert, setInvert] = useState(false);
  const [scope, setScope] = useState<Scope>(initialScope);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string>('');

  useEffect(() => {
    if (!open) return;
    if (initial) {
      setMatchers(initial.matchers.map((m) => ({ ...m })));
      setOutbound(initial.outbound);
      setAction(initial.action);
      setInvert(initial.invert);
    } else {
      setMatchers([{ kind: 'domain_suffix', values: [] }]);
      setOutbound(outboundOptions[0] ?? null);
      setAction(null);
      setInvert(false);
      setScope(initialScope);
    }
    setErr('');
  }, [open, initial, outboundOptions, initialScope]);

  function updateMatcher(idx: number, patch: Partial<Matcher>) {
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
    if (!outbound && (!action || action === 'route')) {
      setErr('Either pick an outbound or set action ≠ route.');
      return;
    }
    if (matchers.length === 0) {
      setErr('At least one matcher is required.');
      return;
    }
    if (matchers.some((m) => m.values.length === 0)) {
      setErr('Each matcher needs at least one value.');
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(
        {
          matchers,
          outbound: action && action !== 'route' ? null : outbound,
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
          <DialogTitle>{initial ? `Edit rule #${initial.id}` : 'New rule'}</DialogTitle>
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
              <p className="text-xs text-muted-foreground">
                Per-config rules attach to the active config. Global rules apply to every
                config you load.
              </p>
            </div>
          )}

          <div className="space-y-2">
            <Label>
              Matchers
              <span className="ml-2 text-xs font-normal text-muted-foreground">
                multi-value within a matcher = OR; across matchers = AND
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
                      {MATCHER_GROUPS.map((g) => (
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
              {matchers.some((m) => !KNOWN_MATCHERS.includes(m.kind as never)) && (
                <Alert>
                  <AlertTitle>Unknown matcher kind</AlertTitle>
                  <AlertDescription>sing-box will likely reject it.</AlertDescription>
                </Alert>
              )}
            </div>
          </div>

          <div className="space-y-2">
            <Label>Action (sing-box ≥ 1.11)</Label>
            <Select
              value={action ?? '__none__'}
              onValueChange={(v) => setAction(v === '__none__' ? null : v)}
            >
              <SelectTrigger className="w-72">
                <SelectValue placeholder="(default: route to outbound)" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">(default: route to outbound)</SelectItem>
                {ACTIONS.map((a) => (
                  <SelectItem key={a} value={a}>
                    {a}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <Label>
              Outbound{!action || action === 'route' ? <span className="text-destructive"> *</span> : null}
            </Label>
            <Select
              value={outbound ?? '__none__'}
              disabled={action != null && action !== 'route'}
              onValueChange={(v) => setOutbound(v === '__none__' ? null : v)}
            >
              <SelectTrigger className="w-96">
                <SelectValue placeholder="select outbound tag" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__">(none)</SelectItem>
                {outboundOptions.map((t) => (
                  <SelectItem key={t} value={t}>
                    {t}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex items-center gap-3">
            <Switch checked={invert} onCheckedChange={setInvert} id="rule-invert" />
            <Label htmlFor="rule-invert" className="cursor-pointer">
              Invert match
              <span className="ml-2 text-xs font-normal text-muted-foreground">
                apply rule when matchers do NOT match
              </span>
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
