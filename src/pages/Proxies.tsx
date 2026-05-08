import {
  Check,
  Gauge,
  Loader2,
  RefreshCw,
  Search,
  Zap,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { proxiesApi, type ProxyEntry } from '../api/proxies';
import { useCoreStore } from '../store/coreStore';
import { useSettingsStore } from '../store/settingsStore';
import { Alert, AlertDescription, AlertTitle } from '../components/ui/alert';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';
import { Input } from '../components/ui/input';
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip';
import { cn } from '../lib/utils';

const TEST_TIMEOUT = 3000;
const SPEEDTEST_BYTES = 10 * 1024 * 1024;
const SPEEDTEST_URL = `https://speed.cloudflare.com/__down?bytes=${SPEEDTEST_BYTES}`;

const SELECTOR_TYPES = ['Selector', 'URLTest', 'Fallback'];

function getHiddenGroupNames(mode: string): Set<string> {
  if (mode === 'global') {
    return new Set(['default', 'DIRECT', 'REJECT', 'COMPATIBLE']);
  }
  return new Set(['GLOBAL', 'default', 'DIRECT', 'REJECT', 'COMPATIBLE']);
}

interface AggregatedDelay {
  [name: string]: number | null;
}
interface AggregatedSpeed {
  [name: string]: number | null;
}

function fmtRate(bps: number | null): string {
  if (bps == null) return '—';
  if (bps < 1024) return `${bps} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KiB/s`;
  return `${(bps / 1024 / 1024).toFixed(2)} MiB/s`;
}

export default function ProxiesPage() {
  const running = useCoreStore((s) => !!s.status?.running);
  const settings = useSettingsStore((s) => s.settings);
  const testUrl = settings?.latency_test_url || 'https://www.gstatic.com/generate_204';

  const [proxies, setProxies] = useState<Record<string, ProxyEntry> | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string>('');
  const [delays, setDelays] = useState<AggregatedDelay>({});
  const [speeds, setSpeeds] = useState<AggregatedSpeed>({});
  const [testingGroup, setTestingGroup] = useState<string>('');
  const [speedTesting, setSpeedTesting] = useState<string>('');
  const [search, setSearch] = useState('');

  async function refresh() {
    if (!running) return;
    setLoading(true);
    setErr('');
    try {
      const r = await proxiesApi.list();
      setProxies(r.proxies);
      const seed: AggregatedDelay = {};
      for (const [name, p] of Object.entries(r.proxies)) {
        const last = p.history?.[p.history.length - 1];
        seed[name] = last?.delay && last.delay > 0 ? last.delay : null;
      }
      setDelays(seed);
    } catch (e) {
      const msg = String((e as Error)?.message ?? e);
      if (/core not running/i.test(msg)) return;
      setErr(msg);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running]);

  async function handleSelect(group: string, name: string) {
    try {
      await proxiesApi.select(group, name);
      toast.success(`${group} → ${name}`);
      const r = await proxiesApi.list();
      setProxies(r.proxies);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }

  async function testGroup(group: ProxyEntry) {
    if (!group.all || group.all.length === 0) return;
    setTestingGroup(group.name);
    try {
      const results = await proxiesApi.testMany(group.all, testUrl, TEST_TIMEOUT);
      setDelays((prev) => {
        const next = { ...prev };
        for (const r of results) next[r.name] = r.delay_ms;
        return next;
      });
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setTestingGroup('');
    }
  }

  async function testOne(name: string) {
    setDelays((prev) => ({ ...prev, [name]: prev[name] ?? null }));
    try {
      const r = await proxiesApi.test(name, testUrl, TEST_TIMEOUT);
      setDelays((prev) => ({ ...prev, [name]: r.delay_ms }));
    } catch {
      setDelays((prev) => ({ ...prev, [name]: null }));
    }
  }

  async function speedTestOne(group: string, name: string) {
    setSpeedTesting(name);
    setSpeeds((prev) => ({ ...prev, [name]: null }));
    try {
      const r = await proxiesApi.speedtest(group, name, SPEEDTEST_URL, SPEEDTEST_BYTES);
      setSpeeds((prev) => ({ ...prev, [name]: r.bytes_per_sec }));
      toast.success(
        `${name}: ${fmtRate(r.bytes_per_sec)} (${(r.bytes / 1024 / 1024).toFixed(1)} MiB in ${r.duration_ms}ms)`
      );
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setSpeedTesting('');
    }
  }

  const mode = settings?.proxy_mode ?? 'rule';

  const groups = useMemo(() => {
    if (!proxies) return [];
    const hidden = getHiddenGroupNames(mode);
    let raw = Object.values(proxies).filter(
      (p) => SELECTOR_TYPES.includes(p.type) && !hidden.has(p.name)
    );
    if (mode === 'global') {
      raw = raw.filter((p) => p.name === 'GLOBAL');
    }
    return raw;
  }, [proxies, mode]);

  const filteredGroups = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return groups;
    return groups
      .map((g) => ({
        ...g,
        all: (g.all ?? []).filter((n) => n.toLowerCase().includes(q)),
      }))
      .filter((g) => (g.all?.length ?? 0) > 0 || g.name.toLowerCase().includes(q));
  }, [groups, search]);

  if (!running) {
    return (
      <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
        Start sing-box from the Dashboard to view proxies.
      </div>
    );
  }

  if (mode === 'direct') {
    return (
      <div className="flex flex-col gap-4">
        <h2 className="text-2xl font-semibold tracking-tight">Proxies</h2>
        <Alert>
          <AlertTitle>Direct mode</AlertTitle>
          <AlertDescription>
            All traffic bypasses every proxy. Switch to Rule or Global on the sidebar to use
            proxies.
          </AlertDescription>
        </Alert>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-2xl font-semibold tracking-tight">Proxies</h2>

      {mode === 'global' && (
        <Alert>
          <AlertTitle>Global mode</AlertTitle>
          <AlertDescription>
            All traffic goes through the GLOBAL group's currently-selected node. Pick a node below.
          </AlertDescription>
        </Alert>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={refresh} disabled={loading}>
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
          Refresh
        </Button>
        <div className="relative">
          <Search
            size={14}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            placeholder="filter node by name..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="h-9 w-80 pl-8"
          />
        </div>
        <span className="ml-auto text-xs text-muted-foreground">
          ⚡ = speedtest ({Math.round(SPEEDTEST_BYTES / 1024 / 1024)} MiB through node)
        </span>
      </div>

      {err && (
        <Alert variant="destructive">
          <AlertTitle>Failed to load proxies</AlertTitle>
          <AlertDescription>{err}</AlertDescription>
        </Alert>
      )}
      {loading && !proxies && (
        <div className="flex h-32 items-center justify-center">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      )}

      {filteredGroups.length === 0 && !loading && (
        <div className="rounded-md border border-border p-8 text-center text-sm text-muted-foreground">
          {mode === 'global'
            ? 'No GLOBAL group found — sing-box should have synthesised one. Restart the core?'
            : 'No selector / urltest / fallback groups in this config.'}
        </div>
      )}

      {filteredGroups.map((g) => (
        <Card key={g.name}>
          <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-3">
            <div className="flex items-center gap-2">
              <CardTitle className="text-base">{g.name}</CardTitle>
              <Badge variant="outline" className="text-xs uppercase">
                {g.type}
              </Badge>
              {g.now && (
                <span className="text-xs text-muted-foreground">
                  active: <code className="text-xs">{g.now}</code>
                </span>
              )}
            </div>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={testingGroup === g.name}
                  onClick={() => testGroup(g)}
                >
                  {testingGroup === g.name ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Zap className="h-4 w-4" />
                  )}
                  Test latency
                </Button>
              </TooltipTrigger>
              <TooltipContent>Test latency of all members ({testUrl})</TooltipContent>
            </Tooltip>
          </CardHeader>
          <CardContent>
            <div className="grid gap-2 [grid-template-columns:repeat(auto-fill,minmax(260px,1fr))]">
              {(g.all ?? []).map((name) => {
                const isActive = g.now === name;
                const delay = delays[name];
                const speed = speeds[name];
                const isSpeedTesting = speedTesting === name;
                return (
                  <div
                    key={name}
                    onClick={() => handleSelect(g.name, name)}
                    className={cn(
                      'flex cursor-pointer items-center justify-between gap-2 rounded-md border px-3 py-2 transition-colors',
                      isActive
                        ? // Bright outline + ring so the picked node is
                          // distinct from the (very subtle) hover state.
                          'border-foreground bg-accent ring-2 ring-foreground/40'
                        : 'border-border hover:bg-accent/50'
                    )}
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-2">
                      {isActive && <Check className="h-3.5 w-3.5 shrink-0 text-foreground" />}
                      <span className="truncate text-sm">{name}</span>
                      {isActive && (
                        <Badge
                          variant="default"
                          className="text-[9px] uppercase tracking-wider"
                        >
                          selected
                        </Badge>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          testOne(name);
                        }}
                        title="re-test latency"
                        className="cursor-pointer"
                      >
                        <DelayBadge delay={delay} />
                      </span>
                      {speed != null && (
                        <span className="text-xs tabular-nums text-foreground">
                          ↓{fmtRate(speed)}
                        </span>
                      )}
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7"
                            disabled={isSpeedTesting}
                            onClick={(e) => {
                              e.stopPropagation();
                              speedTestOne(g.name, name);
                            }}
                          >
                            {isSpeedTesting ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                              <Gauge className="h-3.5 w-3.5" />
                            )}
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>
                          Speed test (downloads ~10 MiB through this node)
                        </TooltipContent>
                      </Tooltip>
                    </div>
                  </div>
                );
              })}
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}

function DelayBadge({ delay }: { delay: number | null | undefined }) {
  if (delay == null) {
    return <span className="text-xs text-muted-foreground">—</span>;
  }
  const cls =
    delay < 200
      ? 'text-foreground'
      : delay < 500
        ? 'text-muted-foreground'
        : 'text-destructive';
  return <span className={cn('text-xs tabular-nums', cls)}>{delay}ms</span>;
}
