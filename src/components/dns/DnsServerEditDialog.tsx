import { useEffect, useState } from 'react';
import {
  KNOWN_DNS_SERVER_TYPES,
  type DnsServerInput,
  type DnsServerType,
  type DnsServerView,
} from '../../api/dns';
import type { Scope } from '../../api/rules';
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
  initial: DnsServerView | null;
  outboundOptions: string[];
  showScopePicker?: boolean;
  initialScope?: Scope;
  onCancel: () => void;
  onSubmit: (input: DnsServerInput, scope: Scope) => Promise<void>;
}

/** Field map: which inputs are relevant to which `type`. */
const FIELDS_BY_TYPE: Record<DnsServerType, ('server' | 'port' | 'path')[]> = {
  udp: ['server', 'port'],
  tcp: ['server', 'port'],
  tls: ['server', 'port'],
  https: ['server', 'port', 'path'],
  quic: ['server', 'port'],
  h3: ['server', 'port', 'path'],
  local: [],
  hosts: [],
  dhcp: [],
  fakeip: [],
};

export default function DnsServerEditDialog({
  open,
  initial,
  outboundOptions,
  showScopePicker = false,
  initialScope = 'per_config',
  onCancel,
  onSubmit,
}: Props) {
  const [tag, setTag] = useState('');
  const [kind, setKind] = useState<DnsServerType>('udp');
  const [server, setServer] = useState('');
  const [serverPort, setServerPort] = useState<string>('');
  const [path, setPath] = useState('');
  const [detour, setDetour] = useState<string | null>(null);
  const [domainResolver, setDomainResolver] = useState<string | null>(null);
  const [domainStrategy, setDomainStrategy] = useState('');
  const [scope, setScope] = useState<Scope>(initialScope);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState('');

  useEffect(() => {
    if (!open) return;
    if (initial) {
      setTag(initial.tag);
      const k = (KNOWN_DNS_SERVER_TYPES as readonly string[]).includes(initial.kind)
        ? (initial.kind as DnsServerType)
        : 'udp';
      setKind(k);
      setServer(initial.server ?? '');
      setServerPort(initial.server_port?.toString() ?? '');
      setPath(initial.path ?? '');
      setDetour(initial.detour);
      setDomainResolver(initial.domain_resolver);
      setDomainStrategy(initial.domain_strategy ?? '');
    } else {
      setTag('');
      setKind('udp');
      setServer('');
      setServerPort('');
      setPath('');
      setDetour(null);
      setDomainResolver(null);
      setDomainStrategy('');
      setScope(initialScope);
    }
    setErr('');
  }, [open, initial, initialScope]);

  const fields = FIELDS_BY_TYPE[kind];

  async function handleOk() {
    setErr('');
    if (!tag.trim()) {
      setErr('Tag is required.');
      return;
    }
    if (fields.includes('server') && !server.trim()) {
      setErr(`Server is required for type=${kind}.`);
      return;
    }
    setSubmitting(true);
    try {
      await onSubmit(
        {
          tag: tag.trim(),
          kind,
          server: fields.includes('server') ? server.trim() : null,
          server_port: fields.includes('port') && serverPort
            ? parseInt(serverPort, 10) || null
            : null,
          path: fields.includes('path') && path ? path.trim() : null,
          detour,
          domain_resolver: domainResolver,
          domain_strategy: domainStrategy.trim() || null,
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
          <DialogTitle>{initial ? `Edit DNS server "${initial.tag}"` : 'New DNS server'}</DialogTitle>
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

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="srv-tag">
                Tag<span className="text-destructive"> *</span>
              </Label>
              <Input id="srv-tag" value={tag} onChange={(e) => setTag(e.target.value)} placeholder="my-dns" />
            </div>
            <div className="space-y-2">
              <Label>Type</Label>
              <Select value={kind} onValueChange={(v) => setKind(v as DnsServerType)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {KNOWN_DNS_SERVER_TYPES.map((t) => (
                    <SelectItem key={t} value={t}>
                      {t}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {fields.includes('server') && (
            <div className="grid grid-cols-3 gap-4">
              <div className="col-span-2 space-y-2">
                <Label htmlFor="srv-host">
                  Server<span className="text-destructive"> *</span>
                </Label>
                <Input
                  id="srv-host"
                  value={server}
                  onChange={(e) => setServer(e.target.value)}
                  placeholder={kind === 'tls' || kind === 'https' ? 'dns.cloudflare.com' : '8.8.8.8'}
                />
              </div>
              {fields.includes('port') && (
                <div className="space-y-2">
                  <Label htmlFor="srv-port">Port</Label>
                  <Input
                    id="srv-port"
                    type="number"
                    min={1}
                    max={65535}
                    value={serverPort}
                    onChange={(e) => setServerPort(e.target.value)}
                    placeholder={kind === 'https' ? '443' : '53'}
                  />
                </div>
              )}
            </div>
          )}

          {fields.includes('path') && (
            <div className="space-y-2">
              <Label htmlFor="srv-path">Path</Label>
              <Input
                id="srv-path"
                value={path}
                onChange={(e) => setPath(e.target.value)}
                placeholder="/dns-query"
              />
            </div>
          )}

          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label>Detour (outbound)</Label>
              <Select
                value={detour ?? '__default__'}
                onValueChange={(v) => setDetour(v === '__default__' ? null : v)}
              >
                <SelectTrigger>
                  <SelectValue placeholder="(default)" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__default__">(default route)</SelectItem>
                  {outboundOptions.map((t) => (
                    <SelectItem key={t} value={t}>
                      {t}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>Domain resolver (tag)</Label>
              <Input
                value={domainResolver ?? ''}
                onChange={(e) => setDomainResolver(e.target.value || null)}
                placeholder="(another DNS server tag)"
              />
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="srv-strategy">Domain strategy</Label>
            <Input
              id="srv-strategy"
              className="w-72"
              value={domainStrategy}
              onChange={(e) => setDomainStrategy(e.target.value)}
              placeholder="prefer_ipv4 / ipv4_only / …"
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
          <Button onClick={handleOk} disabled={submitting}>
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
