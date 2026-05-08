import { Eraser, Search, X } from 'lucide-react';
import { useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import dayjs from 'dayjs';
import { toast } from 'sonner';
import {
  connectionsApi,
  onConnectionsSnapshot,
  type ConnRow,
} from '../api/connections';
import { useEventListener } from '../api/useEventListener';
import { useConnectionsStore, type ClosedConn } from '../store/connectionsStore';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '../components/ui/alert-dialog';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '../components/ui/context-menu';
import { Input } from '../components/ui/input';
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from '../components/ui/sheet';
import { Tabs, TabsList, TabsTrigger } from '../components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip';
import { cn } from '../lib/utils';

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(2)} MiB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GiB`;
}

function fmtAge(start: string): string {
  const t = new Date(start).getTime();
  if (!t) return '—';
  const sec = Math.floor((Date.now() - t) / 1000);
  if (sec < 60) return `${sec}s`;
  if (sec < 3600) return `${Math.floor(sec / 60)}m${sec % 60}s`;
  return `${Math.floor(sec / 3600)}h${Math.floor((sec % 3600) / 60)}m`;
}

export default function ConnectionsPage() {
  const snapshot = useConnectionsStore((s) => s.snapshot);
  const closed = useConnectionsStore((s) => s.closed);
  const ingest = useConnectionsStore((s) => s.ingest);

  const [tab, setTab] = useState<'active' | 'closed'>('active');
  const [search, setSearch] = useState('');
  const [picked, setPicked] = useState<ConnRow | ClosedConn | null>(null);
  const [confirmCloseAll, setConfirmCloseAll] = useState(false);

  useEventListener(() => onConnectionsSnapshot((s) => ingest(s)));

  const baseRows: (ConnRow | ClosedConn)[] =
    tab === 'active' ? snapshot?.connections ?? [] : closed;

  const rows = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return baseRows;
    return baseRows.filter((r) => {
      return (
        r.metadata.host.toLowerCase().includes(q) ||
        r.metadata.destinationIP.toLowerCase().includes(q) ||
        r.metadata.sourceIP.toLowerCase().includes(q) ||
        (r.metadata.process || '').toLowerCase().includes(q) ||
        (r.rule || '').toLowerCase().includes(q) ||
        r.chains.join(',').toLowerCase().includes(q)
      );
    });
  }, [baseRows, search]);

  const parentRef = useRef<HTMLDivElement>(null);
  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 38,
    overscan: 8,
  });

  async function handleClose(r: ConnRow) {
    try {
      await connectionsApi.close(r.id);
      toast.success(`Closed ${r.metadata.host || r.metadata.destinationIP}`);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }

  async function handleCloseAllConfirmed() {
    setConfirmCloseAll(false);
    try {
      await connectionsApi.closeAll();
      toast.success('Closed all connections');
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }

  return (
    <div className="flex h-full flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-semibold tracking-tight">Connections</h2>
        {tab === 'active' && (
          <Button
            variant="destructive"
            size="sm"
            disabled={!snapshot || snapshot.connections.length === 0}
            onClick={() => setConfirmCloseAll(true)}
          >
            <Eraser className="h-4 w-4" />
            Close all
          </Button>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-6">
        <Stat label="Active" value={snapshot?.connections.length ?? 0} />
        <Stat label="↑ total" value={fmtBytes(snapshot?.uploadTotal ?? 0)} />
        <Stat label="↓ total" value={fmtBytes(snapshot?.downloadTotal ?? 0)} />
        <Stat label="memory" value={fmtBytes(snapshot?.memory ?? 0)} />
        <div className="relative ml-auto">
          <Search
            size={14}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            placeholder="search host / IP / process / rule..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="h-9 w-72 pl-8"
          />
        </div>
      </div>

      <Tabs value={tab} onValueChange={(v) => setTab(v as 'active' | 'closed')}>
        <TabsList>
          <TabsTrigger value="active">
            Active ({snapshot?.connections.length ?? 0})
          </TabsTrigger>
          <TabsTrigger value="closed">Closed ({closed.length})</TabsTrigger>
        </TabsList>
      </Tabs>

      <div className="rounded-md border border-border text-xs">
        <div className="grid grid-cols-[60px_1fr_90px_110px_90px_90px_80px_1fr] gap-2 border-b border-border bg-muted/40 px-3 py-2 font-semibold text-muted-foreground">
          <div>net</div>
          <div>host / dst</div>
          <div>src</div>
          <div>process</div>
          <div>↑</div>
          <div>↓</div>
          <div>{tab === 'active' ? 'age' : 'closed'}</div>
          <div>chain ← rule</div>
        </div>
        <div
          ref={parentRef}
          className="min-h-[320px] overflow-auto"
          style={{ height: 'calc(100vh - 380px)' }}
        >
          {rows.length === 0 ? (
            <div className="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
              {tab === 'active' ? 'No active connections' : 'No closed connections yet'}
            </div>
          ) : (
            <div style={{ height: v.getTotalSize(), position: 'relative' }}>
              {v.getVirtualItems().map((vi) => {
                const r = rows[vi.index];
                return (
                  <Row
                    key={vi.key}
                    row={r}
                    top={vi.start}
                    isClosed={tab === 'closed'}
                    onClick={() => setPicked(r)}
                    onClose={tab === 'active' ? () => handleClose(r as ConnRow) : null}
                  />
                );
              })}
            </div>
          )}
        </div>
      </div>

      <Sheet open={!!picked} onOpenChange={(o) => !o && setPicked(null)}>
        <SheetContent side="right" className="w-[560px] sm:max-w-[560px]">
          <SheetHeader>
            <SheetTitle>
              {picked?.metadata.host || picked?.metadata.destinationIP || 'Connection'}
            </SheetTitle>
            <SheetDescription>
              Full snapshot of the selected connection (raw JSON).
            </SheetDescription>
          </SheetHeader>
          {picked && (
            <pre className="mt-4 max-h-[80vh] overflow-auto rounded-md border border-border bg-muted/40 p-3 text-xs">
              {JSON.stringify(picked, null, 2)}
            </pre>
          )}
        </SheetContent>
      </Sheet>

      <AlertDialog open={confirmCloseAll} onOpenChange={setConfirmCloseAll}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Close all {snapshot?.connections.length ?? 0} active connections?
            </AlertDialogTitle>
            <AlertDialogDescription>
              Existing connections will be terminated. New ones can re-open immediately.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleCloseAllConfirmed}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Close all
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="text-lg font-semibold tabular-nums">{value}</span>
    </div>
  );
}

function Row({
  row,
  top,
  isClosed,
  onClick,
  onClose,
}: {
  row: ConnRow | ClosedConn;
  top: number;
  isClosed: boolean;
  onClick: () => void;
  onClose: (() => void) | null;
}) {
  const m = row.metadata;
  // Pick the most informative destination label sing-box gave us.
  // Order: sniffed SNI/Host header → SOCKS-supplied host → remote target →
  // raw destination IP. This avoids "127.0.0.1:7890" in dst when the
  // connection is actually proxied to a real upstream — sing-box on
  // some versions reports the inbound listener as destinationIP for
  // connections that haven't fully resolved yet, and only sniffHost
  // carries the real domain.
  const dstHost =
    m.sniffHost || m.host || m.remoteDestination || m.destinationIP || '?';
  const dst = `${dstHost}:${m.destinationPort}`;

  const ageOrClosed = isClosed
    ? dayjs((row as ClosedConn).closed_at_ms).format('HH:mm:ss')
    : fmtAge(row.start);

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          onClick={onClick}
          style={{ top }}
          className={cn(
            'absolute left-0 right-0 grid cursor-pointer grid-cols-[60px_1fr_90px_110px_90px_90px_80px_1fr] items-center gap-2 border-b border-border/50 px-3 py-2',
            isClosed && 'opacity-65'
          )}
        >
          <Badge variant={m.network === 'udp' ? 'default' : 'secondary'} className="justify-self-start uppercase">
            {m.network || '?'}
          </Badge>
          <span className="truncate text-foreground">{dst}</span>
          <span className="truncate text-muted-foreground">
            {m.sourceIP}:{m.sourcePort}
          </span>
          <span className="truncate text-muted-foreground">{m.process || '—'}</span>
          <span className="text-foreground">{fmtBytes(row.upload)}</span>
          <span className="text-muted-foreground">{fmtBytes(row.download)}</span>
          <span className="text-muted-foreground">{ageOrClosed}</span>
          <div className="flex items-center gap-1">
            <span className="flex-1 truncate text-muted-foreground">
              {row.chains.join(' → ')} {row.rule ? `← ${row.rule}` : ''}
            </span>
            {onClose && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-destructive hover:text-destructive"
                    onClick={(e) => {
                      e.stopPropagation();
                      onClose();
                    }}
                  >
                    <X className="h-3.5 w-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Close connection</TooltipContent>
              </Tooltip>
            )}
          </div>
        </div>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {onClose && (
          <ContextMenuItem onSelect={onClose} className="text-destructive">
            <X className="h-4 w-4" />
            Close connection
          </ContextMenuItem>
        )}
        <ContextMenuItem
          onSelect={() => {
            navigator.clipboard?.writeText(row.id);
            toast.success('id copied');
          }}
        >
          Copy id
        </ContextMenuItem>
        <ContextMenuItem onSelect={onClick}>View details</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
