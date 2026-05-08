import { Loader2, RefreshCcw, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts';
import { Link } from 'react-router-dom';
import { coreApi, type PrivilegeReport } from '../api/core';
import { onCoreState, onPumpStale, onTraffic } from '../api/events';
import { useEventListener } from '../api/useEventListener';
import { useConfigStore } from '../store/configStore';
import { useCoreStore } from '../store/coreStore';
import { useConnectionsStore } from '../store/connectionsStore';
import { useSettingsStore } from '../store/settingsStore';
import { Alert, AlertDescription, AlertTitle } from '../components/ui/alert';
import { Button } from '../components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';

function formatRate(bytes: number): string {
  if (bytes < 1024) return `${bytes} B/s`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB/s`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MiB/s`;
}

export default function Dashboard() {
  const summary = useConfigStore((s) => s.summary);
  const status = useCoreStore((s) => s.status);
  const setStatus = useCoreStore((s) => s.setStatus);
  const traffic = useCoreStore((s) => s.traffic);
  const pushTraffic = useCoreStore((s) => s.pushTraffic);
  const settings = useSettingsStore((s) => s.settings);
  const connSnapshot = useConnectionsStore((s) => s.snapshot);

  const [privilege, setPrivilege] = useState<PrivilegeReport | null>(null);
  const [version, setVersion] = useState<string>('');
  const [restarting, setRestarting] = useState(false);
  const [crash, setCrash] = useState<{ code: number | null; at: number } | null>(null);
  const [stalePumps, setStalePumps] = useState<Set<string>>(new Set());

  useEventListener(() => onTraffic((t) => pushTraffic(t)));
  useEventListener(() =>
    onCoreState((s) => {
      coreApi.status().then(setStatus).catch(() => {});
      if (s.kind === 'crashed') {
        setCrash({ code: s.code, at: Date.now() });
      } else if (s.kind === 'started') {
        setCrash(null);
        setStalePumps(new Set());
      }
    })
  );
  useEventListener(() =>
    onPumpStale((e) =>
      setStalePumps((prev) => {
        const next = new Set(prev);
        next.add(e.kind);
        return next;
      })
    )
  );

  useEffect(() => {
    coreApi.status().then(setStatus).catch(() => {});
    coreApi.checkPrivilege().then(setPrivilege).catch(() => {});
    coreApi.version().then(setVersion).catch(() => setVersion(''));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const running = !!status?.running;
  const last = traffic[traffic.length - 1];
  const latestUp = last?.up ?? 0;
  const latestDown = last?.down ?? 0;

  const chartData = traffic.map((t) => ({
    t: new Date(t.ts_ms).toLocaleTimeString().slice(-8),
    up: t.up,
    down: t.down,
  }));

  async function handleRestart() {
    setRestarting(true);
    try {
      const s = await coreApi.restart();
      setStatus(s);
    } finally {
      setRestarting(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-semibold tracking-tight">Dashboard</h2>
        <Button onClick={handleRestart} disabled={!running || restarting} variant="outline">
          {restarting ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCcw className="h-4 w-4" />
          )}
          Restart core
        </Button>
      </div>

      {crash && (
        <Alert variant="destructive">
          <AlertTitle>
            sing-box exited unexpectedly{crash.code != null ? ` (exit code ${crash.code})` : ''}
          </AlertTitle>
          <AlertDescription>
            The core process died on its own. Click Restart core, or check Logs for the last
            stderr lines.
          </AlertDescription>
        </Alert>
      )}

      {stalePumps.size > 0 && !crash && (
        <Alert>
          <AlertTitle>
            Lost contact with sing-box ({Array.from(stalePumps).join(', ')})
          </AlertTitle>
          <AlertDescription>
            The core may still be running, but our control channel gave up reconnecting. Click
            Restart core.
          </AlertDescription>
        </Alert>
      )}

      {privilege && !privilege.tun_capable && (
        <Alert>
          <ShieldCheck className="h-4 w-4" />
          <AlertTitle>TUN privileges not granted</AlertTitle>
          <AlertDescription>
            {privilege.hint}{' '}
            <span className="text-muted-foreground">
              (configs without TUN inbound — pure SOCKS/HTTP — work without this.)
            </span>
          </AlertDescription>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Core</CardTitle>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="grid grid-cols-3 gap-y-4">
            <Stat label="State" value={running ? 'running' : 'stopped'} />
            <Stat label="↑ upload" value={running ? formatRate(latestUp) : '—'} />
            <Stat label="↓ download" value={running ? formatRate(latestDown) : '—'} />
            <Stat label="connections" value={connSnapshot?.connections.length ?? 0} />
            <Stat label="TUN mode" value={settings?.tun_enabled ? 'enabled' : 'disabled'} />
            <Stat label="version" value={status?.version ?? '—'} />
          </div>

          <div className="h-48">
            <ResponsiveContainer>
              <AreaChart data={chartData} margin={{ top: 10, right: 10, left: 0, bottom: 0 }}>
                <defs>
                  <linearGradient id="gUp" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="hsl(var(--foreground))" stopOpacity={0.5} />
                    <stop offset="100%" stopColor="hsl(var(--foreground))" stopOpacity={0} />
                  </linearGradient>
                  <linearGradient id="gDown" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor="hsl(var(--muted-foreground))" stopOpacity={0.4} />
                    <stop offset="100%" stopColor="hsl(var(--muted-foreground))" stopOpacity={0} />
                  </linearGradient>
                </defs>
                <XAxis dataKey="t" hide />
                <YAxis
                  tickFormatter={formatRate}
                  width={70}
                  stroke="currentColor"
                  className="text-muted-foreground"
                  fontSize={11}
                />
                <Tooltip
                  contentStyle={{
                    background: 'hsl(var(--popover))',
                    border: '1px solid hsl(var(--border))',
                    color: 'hsl(var(--popover-foreground))',
                  }}
                  formatter={(v: number) => formatRate(v)}
                />
                <Area
                  type="monotone"
                  dataKey="up"
                  stroke="currentColor"
                  fill="url(#gUp)"
                  isAnimationActive={false}
                />
                <Area
                  type="monotone"
                  dataKey="down"
                  stroke="currentColor"
                  className="text-muted-foreground"
                  fill="url(#gDown)"
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <CardTitle className="text-base">Active config</CardTitle>
          <Link to="/config">
            <Button variant="link" size="sm">
              {summary ? 'Manage configs' : 'Add config'}
            </Button>
          </Link>
        </CardHeader>
        <CardContent>
          {summary ? (
            <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-sm">
              <KV label="Path" value={<code className="text-xs">{summary.path}</code>} span />
              <KV
                label="Inbounds"
                value={
                  summary.inbounds.length === 0 ? (
                    <span className="text-muted-foreground">none</span>
                  ) : (
                    <div className="flex flex-wrap gap-1">
                      {summary.inbounds.map((i, idx) => (
                        <span
                          key={idx}
                          className="rounded-md border border-border bg-muted px-2 py-0.5 text-xs"
                        >
                          {i.type}
                          {i.tag ? ` / ${i.tag}` : ''}
                        </span>
                      ))}
                    </div>
                  )
                }
              />
              <KV label="Outbounds" value={summary.outbound_tags.length} />
              <KV label="Rules" value={summary.rule_count} />
              <KV
                label="Final outbound"
                value={summary.final_outbound ?? <span className="text-muted-foreground">—</span>}
              />
            </dl>
          ) : (
            <p className="text-sm text-muted-foreground">
              No config selected. Open the Config page to add one.
            </p>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">sing-box binary</CardTitle>
        </CardHeader>
        <CardContent>
          <pre className="overflow-x-auto rounded-md border border-border bg-muted px-3 py-2 text-xs">
            {version || 'probing…'}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="text-xl font-semibold tabular-nums">{value}</span>
    </div>
  );
}

function KV({
  label,
  value,
  span,
}: {
  label: string;
  value: React.ReactNode;
  span?: boolean;
}) {
  return (
    <div className={span ? 'col-span-2 flex flex-col gap-1' : 'flex flex-col gap-1'}>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm">{value}</dd>
    </div>
  );
}
