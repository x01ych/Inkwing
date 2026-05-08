import { Download, Eraser, Pause, Play, Search } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import dayjs from 'dayjs';
import { logsApi, onLogsAppend, type LogEntry } from '../api/logs';
import { useEventListener } from '../api/useEventListener';
import { useLogsStore } from '../store/logsStore';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Checkbox } from '../components/ui/checkbox';
import { Input } from '../components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '../components/ui/popover';
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip';
import { cn } from '../lib/utils';

const LEVEL_VARIANT: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
  debug: 'outline',
  info: 'secondary',
  warn: 'default',
  warning: 'default',
  error: 'destructive',
  fatal: 'destructive',
  panic: 'destructive',
};

const LEVELS = ['debug', 'info', 'warn', 'error', 'fatal'];

export default function LogsPage() {
  const { entries, paused, hydrate, appendBatch, setPaused, clear } = useLogsStore();
  const [levelFilter, setLevelFilter] = useState<string[]>(['info', 'warn', 'error', 'fatal']);
  const [search, setSearch] = useState('');

  // Hydrate from backend ring ONCE if our store is empty (e.g. cold app
  // start). Switching tabs MUST NOT re-hydrate.
  useEffect(() => {
    if (entries.length === 0) {
      logsApi
        .recent()
        .then((es) => hydrate(es))
        .catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEventListener(() => onLogsAppend((b) => appendBatch(b.entries, b.epoch)));

  const filtered = useMemo(() => {
    const set = new Set(levelFilter);
    const q = search.trim().toLowerCase();
    return entries.filter((e) => {
      const lvl = (e.level || 'info').toLowerCase();
      if (!set.has(lvl)) return false;
      if (q && !e.payload.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [entries, levelFilter, search]);

  const parentRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 32,
    overscan: 12,
    // Wrapped log lines (long payloads, whitespace-pre-wrap break-words)
    // are taller than the 32-px estimate. Without measureElement, the
    // next row's `top` is computed from the estimate and overlaps the
    // tail of the wrapped row. measureElement reads the real height
    // post-mount and re-positions everything.
    measureElement:
      typeof window !== 'undefined' && navigator.userAgent.indexOf('Firefox') === -1
        ? (el) => el?.getBoundingClientRect().height
        : undefined,
  });

  // Auto-scroll to bottom on new entries unless user has scrolled up.
  const userScrolledUp = useRef(false);
  useEffect(() => {
    const el = parentRef.current;
    if (!el) return;
    const onScroll = () => {
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      userScrolledUp.current = !atBottom;
    };
    el.addEventListener('scroll', onScroll);
    return () => el.removeEventListener('scroll', onScroll);
  }, []);
  useEffect(() => {
    if (!parentRef.current || userScrolledUp.current) return;
    parentRef.current.scrollTop = parentRef.current.scrollHeight;
  }, [filtered.length]);

  function handleExport() {
    const csv =
      'ts,level,payload\n' +
      filtered
        .map(
          (e) =>
            `${dayjs(e.ts_ms).format('YYYY-MM-DD HH:mm:ss.SSS')},${e.level},"${(
              e.payload || ''
            ).replace(/"/g, '""')}"`
        )
        .join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `sing-box-logs-${dayjs().format('YYYYMMDD-HHmmss')}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  function toggleLevel(l: string) {
    setLevelFilter((prev) =>
      prev.includes(l) ? prev.filter((x) => x !== l) : [...prev, l]
    );
  }

  return (
    <div className="flex h-full flex-col gap-4">
      <h2 className="text-2xl font-semibold tracking-tight">Logs</h2>

      <div className="flex flex-wrap items-center gap-2">
        <Popover>
          <PopoverTrigger asChild>
            <Button variant="outline" size="sm" className="min-w-32 justify-between">
              <span>
                Levels{' '}
                <span className="text-muted-foreground">({levelFilter.length})</span>
              </span>
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-44 p-2" align="start">
            <div className="space-y-1.5">
              {LEVELS.map((l) => (
                <label
                  key={l}
                  className="flex cursor-pointer items-center gap-2 rounded-sm px-1.5 py-1 text-sm hover:bg-accent"
                >
                  <Checkbox
                    checked={levelFilter.includes(l)}
                    onCheckedChange={() => toggleLevel(l)}
                  />
                  <span>{l.toUpperCase()}</span>
                </label>
              ))}
            </div>
          </PopoverContent>
        </Popover>

        <div className="relative">
          <Search
            size={14}
            className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            placeholder="search payload..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="h-9 w-72 pl-8"
          />
        </div>

        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="outline" size="sm" onClick={() => setPaused(!paused)}>
              {paused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
              {paused ? 'Resume' : 'Pause'}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{paused ? 'Resume' : 'Pause'}</TooltipContent>
        </Tooltip>

        <Button variant="outline" size="sm" onClick={clear}>
          <Eraser className="h-4 w-4" />
          Clear
        </Button>

        <Button
          variant="outline"
          size="sm"
          onClick={handleExport}
          disabled={!filtered.length}
        >
          <Download className="h-4 w-4" />
          Export CSV
        </Button>

        <span className="ml-auto text-xs text-muted-foreground">
          {filtered.length} / {entries.length} entries
        </span>
      </div>

      <div
        ref={parentRef}
        className="flex-1 min-h-[360px] overflow-auto rounded-md border border-border bg-muted/30 font-mono text-xs"
        style={{ height: 'calc(100vh - 240px)' }}
      >
        {filtered.length === 0 ? (
          <div className="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
            No log entries (yet)
          </div>
        ) : (
          <div style={{ height: rowVirtualizer.getTotalSize(), position: 'relative' }}>
            {rowVirtualizer.getVirtualItems().map((vi) => {
              const e = filtered[vi.index];
              return (
                <LogRow
                  key={vi.key}
                  entry={e}
                  top={vi.start}
                  index={vi.index}
                  measureRef={rowVirtualizer.measureElement}
                />
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function LogRow({
  entry,
  top,
  index,
  measureRef,
}: {
  entry: LogEntry;
  top: number;
  index: number;
  measureRef: (el: Element | null) => void;
}) {
  const lvl = (entry.level || 'info').toLowerCase();
  return (
    <div
      ref={measureRef}
      data-index={index}
      style={{ top }}
      className={cn(
        'absolute left-0 right-0 flex items-start gap-2 border-b border-border/50 px-3 py-1.5',
        'whitespace-pre-wrap break-words'
      )}
    >
      <span className="min-w-[88px] shrink-0 text-muted-foreground">
        {dayjs(entry.ts_ms).format('HH:mm:ss.SSS')}
      </span>
      <Badge
        variant={LEVEL_VARIANT[lvl] ?? 'outline'}
        className="min-w-[56px] justify-center font-mono text-[10px] uppercase"
      >
        {lvl}
      </Badge>
      <span className="flex-1">{entry.payload}</span>
    </div>
  );
}
