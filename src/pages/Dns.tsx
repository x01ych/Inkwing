import { AlertTriangle, Eye, EyeOff, Pencil, Plus, RotateCcw, Save, Trash2 } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  dnsCommit,
  dnsRulesApi,
  dnsServersApi,
  type DnsRuleInput,
  type DnsRuleViewWithBadge,
  type DnsServerInput,
  type DnsServerViewWithBadge,
} from '../api/dns';
import type { Scope } from '../api/rules';
import { useConfigStore } from '../store/configStore';
import { useCoreStore } from '../store/coreStore';
import DnsRuleEditDialog from '../components/dns/DnsRuleEditDialog';
import DnsServerEditDialog from '../components/dns/DnsServerEditDialog';
import { Alert, AlertDescription, AlertTitle } from '../components/ui/alert';
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
import { Popover, PopoverContent, PopoverTrigger } from '../components/ui/popover';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../components/ui/table';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '../components/ui/tooltip';

type RuleSourceKind = 'config' | 'local_per' | 'local_global';

function SourceBadge({
  source,
  modified,
  masked,
}: {
  source: RuleSourceKind;
  modified: boolean;
  masked: boolean;
}) {
  if (masked) return <Badge variant="outline" className="text-[10px] uppercase">masked</Badge>;
  if (source === 'config') {
    if (modified) return <Badge className="text-[10px] uppercase">modified</Badge>;
    return <Badge variant="outline" className="text-[10px] uppercase">config</Badge>;
  }
  if (source === 'local_global')
    return <Badge variant="secondary" className="text-[10px] uppercase">local · global</Badge>;
  return <Badge variant="secondary" className="text-[10px] uppercase">local</Badge>;
}

export default function DnsPage() {
  const summary = useConfigStore((s) => s.summary);
  const { t } = useTranslation();

  if (!summary) {
    return (
      <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
        No config loaded. Open one from the Config page.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <h2 className="text-2xl font-semibold tracking-tight">{t('nav.dns')}</h2>
      <Tabs defaultValue="servers">
        <TabsList>
          <TabsTrigger value="servers">DNS servers</TabsTrigger>
          <TabsTrigger value="rules">DNS rules</TabsTrigger>
        </TabsList>
        <TabsContent value="servers" className="mt-4">
          <ServersTab />
        </TabsContent>
        <TabsContent value="rules" className="mt-4">
          <RulesTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function ServersTab() {
  const summary = useConfigStore((s) => s.summary)!;
  const running = useCoreStore((s) => !!s.status?.running);
  const [list, setList] = useState<DnsServerViewWithBadge[]>([]);
  const [loadErr, setLoadErr] = useState('');
  const [dirty, setDirty] = useState(false);
  const [editing, setEditing] = useState<DnsServerViewWithBadge | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<DnsServerViewWithBadge | null>(null);

  async function refresh() {
    setLoadErr('');
    try {
      setList(await dnsServersApi.list());
    } catch (e) {
      setLoadErr(String((e as Error)?.message ?? e));
    }
  }
  useEffect(() => {
    refresh();
  }, [summary.path]);

  const outboundOptions = useMemo(
    () =>
      Array.from(new Set([...(summary.outbound_tags ?? []), 'direct', 'block', 'dns-out'])),
    [summary.outbound_tags]
  );

  function openAdd() {
    setEditing(null);
    setEditOpen(true);
  }
  function openEdit(s: DnsServerViewWithBadge) {
    setEditing(s);
    setEditOpen(true);
  }
  async function handleSubmit(input: DnsServerInput, scope: Scope) {
    try {
      if (editing) setList(await dnsServersApi.update(editing.id, input));
      else setList(await dnsServersApi.add(input, scope));
      setDirty(true);
      setEditOpen(false);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleMask(s: DnsServerViewWithBadge) {
    try {
      setList(await dnsServersApi.mask(s.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleUnmask(s: DnsServerViewWithBadge) {
    try {
      setList(await dnsServersApi.unmask(s.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleRevert(s: DnsServerViewWithBadge) {
    try {
      setList(await dnsServersApi.revert(s.original_signature ?? s.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      setList(await dnsServersApi.delete(deleteTarget.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setDeleteTarget(null);
    }
  }
  async function handleCommit(restart: boolean) {
    setCommitting(true);
    try {
      await dnsCommit(restart);
      toast.success(restart ? 'Saved & restarted core' : 'Saved');
      setDirty(false);
      await refresh();
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setCommitting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          DNS servers — <Badge variant="outline" className="mx-0.5 text-[9px]">config</Badge>{' '}
          first, then local appended. Source file is never modified.
        </p>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={openAdd}>
            <Plus className="h-4 w-4" />
            Add server
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={!dirty || committing}
            onClick={() => handleCommit(false)}
          >
            <Save className="h-4 w-4" />
            Save
          </Button>
          <Button
            size="sm"
            disabled={!dirty || !running || committing}
            onClick={() => handleCommit(true)}
          >
            Save & Restart
          </Button>
        </div>
      </div>

      {loadErr && (
        <Alert variant="destructive">
          <AlertTitle>Could not list DNS servers</AlertTitle>
          <AlertDescription>{loadErr}</AlertDescription>
        </Alert>
      )}

      <div className="rounded-md border border-border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-32">Source</TableHead>
              <TableHead>Tag</TableHead>
              <TableHead className="w-24">Type</TableHead>
              <TableHead>Server / Detour</TableHead>
              <TableHead className="w-32">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {list.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="h-20 text-center text-sm text-muted-foreground">
                  No DNS servers
                </TableCell>
              </TableRow>
            ) : (
              list.map((row) => (
                <TableRow key={row.id} className={row.masked ? 'opacity-60' : undefined}>
                  <TableCell>
                    <SourceBadge source={row.source} modified={row.modified} masked={row.masked} />
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{row.view.tag || <em className="text-muted-foreground">no tag</em>}</span>
                      {!row.view.editable && (
                        <Popover>
                          <PopoverTrigger asChild>
                            <Badge variant="outline" className="cursor-pointer gap-1">
                              <AlertTriangle className="h-3 w-3" /> read-only
                            </Badge>
                          </PopoverTrigger>
                          <PopoverContent className="max-w-xl">
                            <pre className="max-h-72 overflow-auto text-xs">{row.view.raw_pretty}</pre>
                          </PopoverContent>
                        </Popover>
                      )}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant="secondary" className="font-mono text-[10px]">
                      {row.view.kind || '?'}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-xs">
                    <div>
                      {row.view.server ? (
                        <code>{row.view.server}{row.view.server_port ? `:${row.view.server_port}` : ''}{row.view.path ?? ''}</code>
                      ) : row.view.address ? (
                        <code>{row.view.address}</code>
                      ) : (
                        <span className="text-muted-foreground">—</span>
                      )}
                    </div>
                    {row.view.detour && (
                      <div className="text-muted-foreground">↪ via {row.view.detour}</div>
                    )}
                  </TableCell>
                  <TableCell>
                    <ServerActions
                      row={row}
                      onEdit={() => openEdit(row)}
                      onDelete={() => setDeleteTarget(row)}
                      onMask={() => handleMask(row)}
                      onUnmask={() => handleUnmask(row)}
                      onRevert={() => handleRevert(row)}
                    />
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      <DnsServerEditDialog
        open={editOpen}
        initial={editing?.view ?? null}
        outboundOptions={outboundOptions}
        showScopePicker={!editing}
        initialScope="per_config"
        onCancel={() => setEditOpen(false)}
        onSubmit={handleSubmit}
      />

      <AlertDialog open={!!deleteTarget} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete local DNS server "{deleteTarget?.view.tag}"?
            </AlertDialogTitle>
            <AlertDialogDescription>
              DNS rules referencing this tag will fail validation.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function ServerActions({
  row,
  onEdit,
  onDelete,
  onMask,
  onUnmask,
  onRevert,
}: {
  row: DnsServerViewWithBadge;
  onEdit: () => void;
  onDelete: () => void;
  onMask: () => void;
  onUnmask: () => void;
  onRevert: () => void;
}) {
  if (row.masked) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onUnmask}>
            <Eye className="h-3.5 w-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Unmask</TooltipContent>
      </Tooltip>
    );
  }
  return (
    <div className="flex items-center gap-1">
      {row.view.editable && (
        <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onEdit}>
          <Pencil className="h-3.5 w-3.5" />
        </Button>
      )}
      {row.source === 'config' &&
        (row.modified ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onRevert}>
                <RotateCcw className="h-3.5 w-3.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Revert</TooltipContent>
          </Tooltip>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onMask}>
                <EyeOff className="h-3.5 w-3.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Mask</TooltipContent>
          </Tooltip>
        ))}
      {row.source !== 'config' && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-destructive hover:text-destructive"
          onClick={onDelete}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- DNS rules

function RulesTab() {
  const summary = useConfigStore((s) => s.summary)!;
  const running = useCoreStore((s) => !!s.status?.running);
  const [rules, setRules] = useState<DnsRuleViewWithBadge[]>([]);
  const [servers, setServers] = useState<DnsServerViewWithBadge[]>([]);
  const [loadErr, setLoadErr] = useState('');
  const [dirty, setDirty] = useState(false);
  const [editing, setEditing] = useState<DnsRuleViewWithBadge | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<DnsRuleViewWithBadge | null>(null);

  async function refresh() {
    setLoadErr('');
    try {
      const [rs, ss] = await Promise.all([dnsRulesApi.list(), dnsServersApi.list()]);
      setRules(rs);
      setServers(ss);
    } catch (e) {
      setLoadErr(String((e as Error)?.message ?? e));
    }
  }
  useEffect(() => {
    refresh();
  }, [summary.path]);

  const serverOptions = useMemo(
    () => servers.map((s) => s.view.tag).filter(Boolean),
    [servers]
  );

  function openAdd() {
    setEditing(null);
    setEditOpen(true);
  }
  function openEdit(r: DnsRuleViewWithBadge) {
    setEditing(r);
    setEditOpen(true);
  }
  async function handleSubmit(input: DnsRuleInput, scope: Scope) {
    try {
      if (editing) setRules(await dnsRulesApi.update(editing.id, input));
      else setRules(await dnsRulesApi.add(input, scope));
      setDirty(true);
      setEditOpen(false);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleMask(r: DnsRuleViewWithBadge) {
    try {
      setRules(await dnsRulesApi.mask(r.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleUnmask(r: DnsRuleViewWithBadge) {
    try {
      setRules(await dnsRulesApi.unmask(r.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleRevert(r: DnsRuleViewWithBadge) {
    try {
      setRules(await dnsRulesApi.revert(r.original_signature ?? r.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      setRules(await dnsRulesApi.delete(deleteTarget.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setDeleteTarget(null);
    }
  }
  async function handleCommit(restart: boolean) {
    setCommitting(true);
    try {
      await dnsCommit(restart);
      toast.success(restart ? 'Saved & restarted core' : 'Saved');
      setDirty(false);
      await refresh();
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setCommitting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          DNS rules — match-action over the DNS query.
        </p>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={openAdd}>
            <Plus className="h-4 w-4" />
            Add rule
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={!dirty || committing}
            onClick={() => handleCommit(false)}
          >
            <Save className="h-4 w-4" />
            Save
          </Button>
          <Button
            size="sm"
            disabled={!dirty || !running || committing}
            onClick={() => handleCommit(true)}
          >
            Save & Restart
          </Button>
        </div>
      </div>

      {loadErr && (
        <Alert variant="destructive">
          <AlertTitle>Could not list DNS rules</AlertTitle>
          <AlertDescription>{loadErr}</AlertDescription>
        </Alert>
      )}

      <div className="flex flex-col gap-1.5">
        {rules.length === 0 && !loadErr && (
          <div className="rounded-md border border-dashed border-border p-8 text-center text-sm text-muted-foreground">
            No DNS rules
          </div>
        )}
        {rules.map((r, idx) => (
          <DnsRuleRow
            key={r.id}
            rule={r}
            idx={idx}
            onEdit={() => openEdit(r)}
            onDelete={() => setDeleteTarget(r)}
            onMask={() => handleMask(r)}
            onUnmask={() => handleUnmask(r)}
            onRevert={() => handleRevert(r)}
          />
        ))}
      </div>

      <DnsRuleEditDialog
        open={editOpen}
        initial={editing?.view ?? null}
        serverOptions={serverOptions}
        showScopePicker={!editing}
        initialScope="per_config"
        onCancel={() => setEditOpen(false)}
        onSubmit={handleSubmit}
      />

      <AlertDialog open={!!deleteTarget} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this local DNS rule?</AlertDialogTitle>
            <AlertDialogDescription asChild>
              <pre className="max-h-60 overflow-auto rounded-md border border-border bg-muted/40 p-2 text-xs">
                {deleteTarget?.view.raw_pretty}
              </pre>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={confirmDelete}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function DnsRuleRow({
  rule,
  idx,
  onEdit,
  onDelete,
  onMask,
  onUnmask,
  onRevert,
}: {
  rule: DnsRuleViewWithBadge;
  idx: number;
  onEdit: () => void;
  onDelete: () => void;
  onMask: () => void;
  onUnmask: () => void;
  onRevert: () => void;
}) {
  const v = rule.view;
  return (
    <div
      className={
        'flex items-center gap-2 rounded-md border border-border bg-card px-3 py-2' +
        (rule.masked ? ' border-dashed opacity-50' : '')
      }
    >
      <span className="min-w-[28px] text-xs text-muted-foreground">#{idx}</span>
      <SourceBadge source={rule.source} modified={rule.modified} masked={rule.masked} />
      {v.editable ? (
        <>
          <div className="flex flex-1 flex-wrap gap-1">
            {v.matchers.map((m, i) => (
              <Badge key={i} variant="secondary" className="font-mono text-[11px]">
                {m.kind}:{' '}
                {m.values.length <= 3
                  ? m.values.join(',')
                  : `${m.values.slice(0, 2).join(',')}, +${m.values.length - 2}`}
              </Badge>
            ))}
            {v.invert && <Badge variant="outline">invert</Badge>}
          </div>
          <Badge className="font-mono">
            → {v.action && v.action !== 'route' ? v.action : v.server ?? '?'}
          </Badge>
        </>
      ) : (
        <>
          <Popover>
            <PopoverTrigger asChild>
              <Badge variant="outline" className="cursor-pointer gap-1">
                <AlertTriangle className="h-3 w-3" /> read-only
              </Badge>
            </PopoverTrigger>
            <PopoverContent side="left" className="max-w-2xl">
              <pre className="max-h-80 overflow-auto text-xs">{v.raw_pretty}</pre>
            </PopoverContent>
          </Popover>
          <span className="flex-1 truncate text-xs text-muted-foreground">{v.readonly_reason}</span>
        </>
      )}
      {rule.masked ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onUnmask}>
              <Eye className="h-3.5 w-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Unmask</TooltipContent>
        </Tooltip>
      ) : (
        <>
          {v.editable && (
            <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onEdit}>
              <Pencil className="h-3.5 w-3.5" />
            </Button>
          )}
          {rule.source === 'config' &&
            (rule.modified ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onRevert}>
                    <RotateCcw className="h-3.5 w-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Revert</TooltipContent>
              </Tooltip>
            ) : (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onMask}>
                    <EyeOff className="h-3.5 w-3.5" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Mask</TooltipContent>
              </Tooltip>
            ))}
          {rule.source !== 'config' && (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 text-destructive hover:text-destructive"
              onClick={onDelete}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          )}
        </>
      )}
    </div>
  );
}
