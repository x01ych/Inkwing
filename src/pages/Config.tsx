import {
  Check,
  CloudDownload,
  FileText,
  FolderOpen,
  MoreHorizontal,
  Pencil,
  Plus,
  RefreshCcw,
  RotateCcw,
  Trash2,
} from 'lucide-react';
import dayjs from 'dayjs';
import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import { configApi, type ConfigEntrySummary } from '../api/config';
import { coreApi } from '../api/core';
import { subsApi, type Subscription } from '../api/subscriptions';
import { onCoreState } from '../api/events';
import { useEventListener } from '../api/useEventListener';
import { useConfigStore } from '../store/configStore';
import { useCoreStore } from '../store/coreStore';
import AddFromSubscriptionDialog from '../components/config/AddFromSubscriptionDialog';
import EditSubscriptionDialog from '../components/config/EditSubscriptionDialog';
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
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../components/ui/dialog';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '../components/ui/context-menu';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '../components/ui/dropdown-menu';
import { Input } from '../components/ui/input';
import { Label } from '../components/ui/label';
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip';
import { cn } from '../lib/utils';

export default function ConfigPage() {
  const navigate = useNavigate();
  const status = useCoreStore((s) => s.status);
  const setStatus = useCoreStore((s) => s.setStatus);
  const running = !!status?.running;

  const [library, setLibrary] = useState<ConfigEntrySummary[]>([]);
  const [subs, setSubs] = useState<Subscription[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyId, setBusyId] = useState<string>('');
  const [restarting, setRestarting] = useState(false);
  const [addSubOpen, setAddSubOpen] = useState(false);
  const [editSub, setEditSub] = useState<Subscription | null>(null);
  const [renameTarget, setRenameTarget] = useState<ConfigEntrySummary | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<ConfigEntrySummary | null>(null);

  async function refresh() {
    setLoading(true);
    try {
      const [libs, ss] = await Promise.all([configApi.libraryList(), subsApi.list()]);
      setLibrary(libs);
      setSubs(ss);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  useEventListener(() =>
    onCoreState(() => coreApi.status().then(setStatus).catch(() => {}))
  );

  async function pickAndAddLocal() {
    try {
      const path = await configApi.openDialog();
      if (!path) return;
      await configApi.libraryAddLocal(path);
      toast.success('Added to library');
      await refresh();
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }

  async function handleSelect(row: ConfigEntrySummary) {
    if (row.is_active) return;
    setBusyId(row.id);
    try {
      const summary = await configApi.librarySelect(row.id);
      useConfigStore.getState().setSummary(summary);
      toast.success(`Active: "${row.name}" — sing-box started/restarted`);
      const [libs, st] = await Promise.all([configApi.libraryList(), coreApi.status()]);
      setLibrary(libs);
      setStatus(st);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setBusyId('');
    }
  }

  function handleRenameOpen(row: ConfigEntrySummary) {
    setRenameTarget(row);
    setRenameValue(row.name);
  }
  async function handleRenameSubmit() {
    if (!renameTarget) return;
    try {
      await configApi.libraryRename(renameTarget.id, renameValue.trim() || renameTarget.name);
      await refresh();
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setRenameTarget(null);
    }
  }

  function handleEditSubscription(row: ConfigEntrySummary) {
    const src = row.source;
    if (src.kind !== 'subscription') {
      toast.info('This config is not from a subscription.');
      return;
    }
    const target = subs.find((s) => s.id === src.sub_id);
    if (!target) {
      toast.error('Source subscription no longer exists.');
      return;
    }
    setEditSub(target);
  }

  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      await configApi.libraryRemove(deleteTarget.id);
      await refresh();
      setStatus(await coreApi.status());
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setDeleteTarget(null);
    }
  }

  async function handleRefreshSubscriptionEntry(row: ConfigEntrySummary) {
    if (row.source.kind !== 'subscription') {
      toast.info('Only subscription-sourced configs can refresh.');
      return;
    }
    setBusyId(row.id);
    try {
      await configApi.libraryRefreshFromSubscription(row.id);
      toast.success(`Refreshed "${row.name}"${row.is_active ? ' — core restarted' : ''}`);
      const [libs, st] = await Promise.all([configApi.libraryList(), coreApi.status()]);
      setLibrary(libs);
      setStatus(st);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setBusyId('');
    }
  }

  async function handleRestart() {
    setRestarting(true);
    try {
      const s = await coreApi.restart();
      setStatus(s);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setRestarting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-semibold tracking-tight">Config</h2>
        <div className="flex items-center gap-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="outline"
                size="sm"
                disabled={!running || restarting}
                onClick={handleRestart}
              >
                <RotateCcw className={cn('h-4 w-4', restarting && 'animate-spin')} />
                Restart
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              Restart sing-box (re-applies TUN / port overlays + active config)
            </TooltipContent>
          </Tooltip>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm">
                <Plus className="h-4 w-4" />
                Add config
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={pickAndAddLocal}>
                <FileText className="h-4 w-4" />
                Add local file…
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setAddSubOpen(true)}>
                <CloudDownload className="h-4 w-4" />
                Add from subscription URL…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      {library.length === 0 && !loading ? (
        <div className="rounded-md border border-dashed border-border p-12 text-center text-sm text-muted-foreground">
          No configs in your library — click 'Add config' to start.
        </div>
      ) : (
        <div className="grid gap-3 [grid-template-columns:repeat(auto-fill,minmax(280px,1fr))]">
          {library.map((row) => (
            <ConfigCard
              key={row.id}
              row={row}
              busy={busyId === row.id}
              onSelect={() => handleSelect(row)}
              onRename={() => handleRenameOpen(row)}
              onEditSub={() => handleEditSubscription(row)}
              onReveal={() =>
                configApi.libraryReveal(row.id).catch((e) => toast.error(String(e)))
              }
              onView={() => navigate(`/config/${row.id}`)}
              onRefresh={() => handleRefreshSubscriptionEntry(row)}
              onDelete={() => setDeleteTarget(row)}
            />
          ))}
        </div>
      )}

      <AddFromSubscriptionDialog
        open={addSubOpen}
        onCancel={() => setAddSubOpen(false)}
        onAdded={async () => {
          setAddSubOpen(false);
          await refresh();
        }}
      />
      <EditSubscriptionDialog
        open={!!editSub}
        initial={editSub}
        onCancel={() => setEditSub(null)}
        onSaved={async () => {
          setEditSub(null);
          await refresh();
        }}
      />

      <Dialog
        open={!!renameTarget}
        onOpenChange={(o) => !o && setRenameTarget(null)}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Rename "{renameTarget?.name}"</DialogTitle>
          </DialogHeader>
          <div className="py-2">
            <Label htmlFor="rename-input" className="sr-only">
              Name
            </Label>
            <Input
              id="rename-input"
              autoFocus
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleRenameSubmit()}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenameTarget(null)}>
              Cancel
            </Button>
            <Button onClick={handleRenameSubmit}>Rename</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove "{deleteTarget?.name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              The managed copy at <code className="text-xs">{deleteTarget?.storage_path}</code>{' '}
              will be deleted.
              {deleteTarget?.is_active &&
                ' This is the active config — sing-box will switch (or stop) automatically.'}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function ConfigCard({
  row,
  busy,
  onSelect,
  onRename,
  onEditSub,
  onReveal,
  onView,
  onRefresh,
  onDelete,
}: {
  row: ConfigEntrySummary;
  busy: boolean;
  onSelect: () => void;
  onRename: () => void;
  onEditSub: () => void;
  onReveal: () => void;
  onView: () => void;
  onRefresh: () => void;
  onDelete: () => void;
}) {
  const isSub = row.source.kind === 'subscription';

  // Items shared by both menus (right-click ContextMenu and ⋯ DropdownMenu).
  // Each entry is keyed and renders into the matching primitive below.
  const items: {
    key: string;
    label: string;
    icon: React.ReactNode;
    onSelect: () => void;
    disabled?: boolean;
    danger?: boolean;
    sep?: boolean;
  }[] = [
    { key: 'rename', label: 'Edit info (rename)', icon: <Pencil className="h-4 w-4" />, onSelect: onRename },
    { key: 'editsub', label: 'Edit subscription', icon: <CloudDownload className="h-4 w-4" />, onSelect: onEditSub, disabled: !isSub },
    { key: 'view', label: 'View raw', icon: <FileText className="h-4 w-4" />, onSelect: onView },
    { key: 'reveal', label: 'Open file location', icon: <FolderOpen className="h-4 w-4" />, onSelect: onReveal },
    { key: '__sep__', label: '', icon: null, onSelect: () => {}, sep: true },
    { key: 'delete', label: 'Delete', icon: <Trash2 className="h-4 w-4" />, onSelect: onDelete, danger: true },
  ];

  const ContextItems = (
    <ContextMenuContent>
      {items.map((it) =>
        it.sep ? (
          <ContextMenuSeparator key={it.key} />
        ) : (
          <ContextMenuItem
            key={it.key}
            disabled={it.disabled}
            onSelect={it.onSelect}
            className={it.danger ? 'text-destructive' : undefined}
          >
            {it.icon}
            {it.label}
          </ContextMenuItem>
        )
      )}
    </ContextMenuContent>
  );
  const DropdownItems = (
    <DropdownMenuContent align="end">
      {items.map((it) =>
        it.sep ? (
          <DropdownMenuSeparator key={it.key} />
        ) : (
          <DropdownMenuItem
            key={it.key}
            disabled={it.disabled}
            onClick={it.onSelect}
            className={it.danger ? 'text-destructive' : undefined}
          >
            {it.icon}
            {it.label}
          </DropdownMenuItem>
        )
      )}
    </DropdownMenuContent>
  );

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <div
          onClick={onSelect}
          className={cn(
            'flex min-h-[150px] flex-col gap-2.5 rounded-md border p-4 transition-colors',
            row.is_active
              ? // Bright outline + accent fill so the active card pops
                // out of a grid; ring sits outside the border to make
                // the bracket effect read at a glance.
                'border-foreground bg-accent ring-2 ring-foreground/40 cursor-default'
              : 'border-border bg-card cursor-pointer hover:bg-accent/40',
            busy && 'opacity-60'
          )}
        >
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                {row.is_active && <Check className="h-3.5 w-3.5 shrink-0 text-foreground" />}
                <span className="truncate text-sm font-medium">{row.name}</span>
              </div>
              <div className="mt-1 flex flex-wrap gap-1">
                {row.is_active && (
                  // Explicit "active" pill — colour-only signals fail
                  // when accent / muted / secondary all collapse to
                  // the same dark grey in the default zinc theme.
                  <Badge variant="default" className="text-[10px] uppercase tracking-wider">
                    active
                  </Badge>
                )}
                <Badge variant={isSub ? 'secondary' : 'outline'} className="text-[10px]">
                  {isSub ? 'subscription' : 'local'}
                </Badge>
                {row.has_tun_inbound && (
                  <Badge variant="default" className="text-[10px]">
                    tun
                  </Badge>
                )}
              </div>
            </div>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7"
                  onClick={(e) => e.stopPropagation()}
                >
                  <MoreHorizontal className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              {DropdownItems}
            </DropdownMenu>
          </div>

          <div className="flex gap-6">
            <div>
              <div className="text-[11px] text-muted-foreground">outbounds</div>
              <div className="text-base font-medium tabular-nums">
                {row.outbound_count ?? '—'}
              </div>
            </div>
            <div>
              <div className="text-[11px] text-muted-foreground">rules</div>
              <div className="text-base font-medium tabular-nums">
                {row.rule_count ?? '—'}
              </div>
            </div>
          </div>

          <div className="text-[11px] text-muted-foreground">
            updated {dayjs(row.updated_at_ms).format('MM-DD HH:mm')}
          </div>

          <div className="mt-auto flex justify-end gap-2">
            {isSub && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={(e) => {
                      e.stopPropagation();
                      onRefresh();
                    }}
                  >
                    <RefreshCcw className={cn('h-3.5 w-3.5', busy && 'animate-spin')} />
                    Refresh
                  </Button>
                </TooltipTrigger>
                <TooltipContent>
                  Re-fetch this entry from its source subscription
                </TooltipContent>
              </Tooltip>
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={(e) => {
                e.stopPropagation();
                onView();
              }}
            >
              <FileText className="h-3.5 w-3.5" />
              View
            </Button>
          </div>
        </div>
      </ContextMenuTrigger>
      {ContextItems}
    </ContextMenu>
  );
}
