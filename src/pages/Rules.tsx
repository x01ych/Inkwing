import {
  AlertTriangle,
  Eye,
  EyeOff,
  GripVertical,
  Pencil,
  Plus,
  RotateCcw,
  Save,
  Trash2,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ruleSetsApi,
  rulesApi,
  type RuleSetInput,
  type RuleSetViewWithBadge,
  type RuleViewWithBadge,
  type Scope,
  type RuleInput,
} from '../api/rules';
import { useConfigStore } from '../store/configStore';
import { useCoreStore } from '../store/coreStore';
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import RuleEditDialog from '../components/rules/RuleEditDialog';
import RuleSetEditDialog from '../components/rules/RuleSetEditDialog';
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
import { cn } from '../lib/utils';

export default function RulesPage() {
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
      <h2 className="text-2xl font-semibold tracking-tight">{t('nav.route')}</h2>
      <Tabs defaultValue="rules">
        <TabsList>
          <TabsTrigger value="rules">Routing rules</TabsTrigger>
          <TabsTrigger value="rulesets">Rule sets</TabsTrigger>
        </TabsList>
        <TabsContent value="rules" className="mt-4">
          <RulesTab />
        </TabsContent>
        <TabsContent value="rulesets" className="mt-4">
          <RuleSetsTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}

// ---------------------------------------------------------------- helpers

function SourceBadge({
  source,
  modified,
  masked,
}: {
  source: 'config' | 'local_per' | 'local_global';
  modified: boolean;
  masked: boolean;
}) {
  if (masked) {
    return (
      <Badge variant="outline" className="text-[10px] uppercase">
        masked
      </Badge>
    );
  }
  if (source === 'config') {
    if (modified) {
      return <Badge className="text-[10px] uppercase">modified</Badge>;
    }
    return (
      <Badge variant="outline" className="text-[10px] uppercase">
        config
      </Badge>
    );
  }
  if (source === 'local_global') {
    return (
      <Badge variant="secondary" className="text-[10px] uppercase">
        local · global
      </Badge>
    );
  }
  return (
    <Badge variant="secondary" className="text-[10px] uppercase">
      local
    </Badge>
  );
}

// ---------------------------------------------------------------- rules

function RulesTab() {
  const summary = useConfigStore((s) => s.summary)!;
  const running = useCoreStore((s) => !!s.status?.running);
  const [rules, setRules] = useState<RuleViewWithBadge[]>([]);
  const [loadErr, setLoadErr] = useState('');
  const [dirty, setDirty] = useState(false);
  const [editing, setEditing] = useState<RuleViewWithBadge | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<RuleViewWithBadge | null>(null);

  async function refresh() {
    setLoadErr('');
    try {
      setRules(await rulesApi.list());
    } catch (e) {
      setLoadErr(String((e as Error)?.message ?? e));
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
  function openEdit(r: RuleViewWithBadge) {
    setEditing(r);
    setEditOpen(true);
  }
  async function handleSubmit(input: RuleInput, scope: Scope) {
    try {
      if (editing) {
        setRules(await rulesApi.update(editing.id, input));
      } else {
        setRules(await rulesApi.add(input, scope));
      }
      setDirty(true);
      setEditOpen(false);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleMask(r: RuleViewWithBadge) {
    try {
      setRules(await rulesApi.mask(r.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleUnmask(r: RuleViewWithBadge) {
    try {
      setRules(await rulesApi.unmask(r.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleRevert(r: RuleViewWithBadge) {
    try {
      setRules(await rulesApi.revert(r.original_signature ?? r.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      setRules(await rulesApi.delete(deleteTarget.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setDeleteTarget(null);
    }
  }

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  async function handleDragEnd(e: DragEndEvent) {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    // Both endpoints must be local rules in the same scope. Source rules
    // live in the source file order — refuse cross-boundary drags.
    const a = rules.find((r) => r.id === active.id);
    const b = rules.find((r) => r.id === over.id);
    if (!a || !b || a.source === 'config' || b.source === 'config' || a.source !== b.source) {
      toast.message('Reorder only works within the same local scope.');
      return;
    }
    const oldIdx = rules.findIndex((r) => r.id === active.id);
    const newIdx = rules.findIndex((r) => r.id === over.id);
    const moved = arrayMove(rules, oldIdx, newIdx);
    setRules(moved);
    try {
      const localIds = moved
        .filter((r) => r.source !== 'config')
        .map((r) => r.id);
      setRules(await rulesApi.reorder(localIds));
      setDirty(true);
    } catch (err) {
      toast.error(String((err as Error)?.message ?? err));
      refresh();
    }
  }

  async function handleCommit(restart: boolean) {
    setCommitting(true);
    try {
      await rulesApi.commit(restart);
      toast.success(restart ? 'Saved & restarted core' : 'Saved (restart manually)');
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
          Match order is top-down. Rules are merged: <Badge variant="outline" className="mx-0.5 text-[9px]">config</Badge>{' '}
          first, then{' '}
          <Badge variant="secondary" className="mx-0.5 text-[9px]">local</Badge>{' '}
          (per-config), then{' '}
          <Badge variant="secondary" className="mx-0.5 text-[9px]">local · global</Badge>.
          Source file is never modified.
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
          <AlertTitle>Could not list rules</AlertTitle>
          <AlertDescription>{loadErr}</AlertDescription>
        </Alert>
      )}

      {dirty && (
        <Alert>
          <AlertTitle>Unsaved changes — restart sing-box for them to take effect.</AlertTitle>
          <AlertDescription>
            Local edits live under <code className="text-xs">~/data/overrides</code>; your{' '}
            <code className="text-xs">{summary.path}</code> source file is never modified by
            this editor.
          </AlertDescription>
        </Alert>
      )}

      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
        <SortableContext items={rules.map((r) => r.id)} strategy={verticalListSortingStrategy}>
          <div className="flex flex-col gap-1.5">
            {rules.map((r, idx) => (
              <RuleRow
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
        </SortableContext>
      </DndContext>

      {summary.final_outbound && (
        <div className="rounded-md border border-dashed border-border bg-muted/30 px-3 py-2 text-sm">
          <span className="text-muted-foreground">Final fallback (no rule matched): </span>
          <Badge variant="outline">{summary.final_outbound}</Badge>
        </div>
      )}

      <RuleEditDialog
        open={editOpen}
        initial={editing?.view ?? null}
        outboundOptions={outboundOptions}
        showScopePicker={!editing}
        initialScope="per_config"
        onCancel={() => setEditOpen(false)}
        onSubmit={handleSubmit}
      />

      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this local rule?</AlertDialogTitle>
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

function RuleRow({
  rule,
  idx,
  onEdit,
  onDelete,
  onMask,
  onUnmask,
  onRevert,
}: {
  rule: RuleViewWithBadge;
  idx: number;
  onEdit: () => void;
  onDelete: () => void;
  onMask: () => void;
  onUnmask: () => void;
  onRevert: () => void;
}) {
  const isLocal = rule.source !== 'config';
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } =
    useSortable({ id: rule.id, disabled: !isLocal });
  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.6 : rule.masked ? 0.45 : 1,
  };
  const v = rule.view;
  return (
    <div
      ref={setNodeRef}
      style={style}
      className={cn(
        'flex items-center gap-2 rounded-md border border-border bg-card px-3 py-2',
        rule.masked && 'border-dashed'
      )}
    >
      <span
        {...attributes}
        {...listeners}
        className={cn(
          'text-muted-foreground',
          isLocal ? 'cursor-grab' : 'cursor-not-allowed opacity-30'
        )}
        title={isLocal ? 'drag to reorder (local rules only)' : 'config rules keep source-file order'}
      >
        <GripVertical size={14} />
      </span>
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
            → {v.action && v.action !== 'route' ? v.action : v.outbound}
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
          <span className="flex-1 truncate text-xs text-muted-foreground">
            {v.readonly_reason}
          </span>
        </>
      )}

      <RowActions
        rule={rule}
        onEdit={onEdit}
        onDelete={onDelete}
        onMask={onMask}
        onUnmask={onUnmask}
        onRevert={onRevert}
      />
    </div>
  );
}

function RowActions({
  rule,
  onEdit,
  onDelete,
  onMask,
  onUnmask,
  onRevert,
}: {
  rule: RuleViewWithBadge;
  onEdit: () => void;
  onDelete: () => void;
  onMask: () => void;
  onUnmask: () => void;
  onRevert: () => void;
}) {
  // Decision matrix:
  //   masked            → unmask
  //   config + modified → edit, revert
  //   config            → edit (will demote to mod), mask
  //   local             → edit, delete
  if (rule.masked) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onUnmask}>
            <Eye className="h-3.5 w-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Unmask (re-enable this config rule)</TooltipContent>
      </Tooltip>
    );
  }
  const buttons: React.ReactNode[] = [];
  if (rule.view.editable) {
    buttons.push(
      <Tooltip key="edit">
        <TooltipTrigger asChild>
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onEdit}>
            <Pencil className="h-3.5 w-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          {rule.source === 'config' ? 'Edit (creates local override)' : 'Edit'}
        </TooltipContent>
      </Tooltip>
    );
  }
  if (rule.source === 'config') {
    if (rule.modified) {
      buttons.push(
        <Tooltip key="revert">
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onRevert}>
              <RotateCcw className="h-3.5 w-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Revert to source rule</TooltipContent>
        </Tooltip>
      );
    } else {
      buttons.push(
        <Tooltip key="mask">
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="h-7 w-7" onClick={onMask}>
              <EyeOff className="h-3.5 w-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Mask (hide this rule from sing-box)</TooltipContent>
        </Tooltip>
      );
    }
  } else {
    // local
    buttons.push(
      <Button
        key="del"
        variant="ghost"
        size="icon"
        className="h-7 w-7 text-destructive hover:text-destructive"
        onClick={onDelete}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </Button>
    );
  }
  return <>{buttons}</>;
}

// ---------------------------------------------------------------- rule_sets

function RuleSetsTab() {
  const summary = useConfigStore((s) => s.summary)!;
  const running = useCoreStore((s) => !!s.status?.running);
  const [list, setList] = useState<RuleSetViewWithBadge[]>([]);
  const [loadErr, setLoadErr] = useState('');
  const [dirty, setDirty] = useState(false);
  const [editing, setEditing] = useState<RuleSetViewWithBadge | null>(null);
  const [editOpen, setEditOpen] = useState(false);
  const [committing, setCommitting] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<RuleSetViewWithBadge | null>(null);

  async function refresh() {
    setLoadErr('');
    try {
      setList(await ruleSetsApi.list());
    } catch (e) {
      setLoadErr(String((e as Error)?.message ?? e));
    }
  }
  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
  function openEdit(rs: RuleSetViewWithBadge) {
    setEditing(rs);
    setEditOpen(true);
  }
  async function handleSubmit(input: RuleSetInput, scope: Scope) {
    try {
      if (editing) {
        setList(await ruleSetsApi.update(editing.id, input));
      } else {
        setList(await ruleSetsApi.add(input, scope));
      }
      setDirty(true);
      setEditOpen(false);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function confirmDelete() {
    if (!deleteTarget) return;
    try {
      setList(await ruleSetsApi.delete(deleteTarget.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setDeleteTarget(null);
    }
  }
  async function handleMask(rs: RuleSetViewWithBadge) {
    try {
      setList(await ruleSetsApi.mask(rs.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleUnmask(rs: RuleSetViewWithBadge) {
    try {
      setList(await ruleSetsApi.unmask(rs.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }
  async function handleRevert(rs: RuleSetViewWithBadge) {
    try {
      setList(await ruleSetsApi.revert(rs.original_signature ?? rs.id));
      setDirty(true);
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    }
  }

  async function handleCommit(restart: boolean) {
    setCommitting(true);
    try {
      await ruleSetsApi.commit(restart);
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
          Reusable rule-set definitions referenced by{' '}
          <code className="text-xs">rule_set</code> matchers in Routing rules.
        </p>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={openAdd}>
            <Plus className="h-4 w-4" />
            Add rule_set
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
          <AlertTitle>Could not list rule_sets</AlertTitle>
          <AlertDescription>{loadErr}</AlertDescription>
        </Alert>
      )}

      {dirty && (
        <Alert>
          <AlertTitle>Unsaved changes — restart sing-box for them to take effect.</AlertTitle>
        </Alert>
      )}

      <div className="rounded-md border border-border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-32">Source</TableHead>
              <TableHead>Tag</TableHead>
              <TableHead className="w-24">Type</TableHead>
              <TableHead className="w-24">Format</TableHead>
              <TableHead>Source URL/Path</TableHead>
              <TableHead className="w-24">Update</TableHead>
              <TableHead className="w-32">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {list.length === 0 ? (
              <TableRow>
                <TableCell colSpan={7} className="h-24 text-center text-sm text-muted-foreground">
                  No rule_set entries
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
                      <span className="font-medium">{row.view.tag}</span>
                      {!row.view.editable && (
                        <Popover>
                          <PopoverTrigger asChild>
                            <Badge variant="outline" className="cursor-pointer gap-1">
                              <AlertTriangle className="h-3 w-3" /> read-only
                            </Badge>
                          </PopoverTrigger>
                          <PopoverContent className="max-w-xl">
                            <pre className="max-h-72 overflow-auto text-xs">
                              {row.view.raw_pretty}
                            </pre>
                          </PopoverContent>
                        </Popover>
                      )}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant={row.view.kind ? 'secondary' : 'outline'}>
                      {row.view.kind || '?'}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-sm">{row.view.format}</TableCell>
                  <TableCell>
                    <code className="block max-w-[360px] truncate text-xs">
                      {row.view.kind === 'remote'
                        ? row.view.url || '(no url)'
                        : row.view.path || '(no path)'}
                    </code>
                  </TableCell>
                  <TableCell className="text-sm">
                    {row.view.update_interval || (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-1">
                      {row.masked ? (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              onClick={() => handleUnmask(row)}
                            >
                              <Eye className="h-3.5 w-3.5" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>Unmask</TooltipContent>
                        </Tooltip>
                      ) : (
                        <>
                          {row.view.editable && (
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7"
                              onClick={() => openEdit(row)}
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                          )}
                          {row.source === 'config' &&
                            (row.modified ? (
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className="h-7 w-7"
                                    onClick={() => handleRevert(row)}
                                  >
                                    <RotateCcw className="h-3.5 w-3.5" />
                                  </Button>
                                </TooltipTrigger>
                                <TooltipContent>Revert</TooltipContent>
                              </Tooltip>
                            ) : (
                              <Tooltip>
                                <TooltipTrigger asChild>
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className="h-7 w-7"
                                    onClick={() => handleMask(row)}
                                  >
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
                              onClick={() => setDeleteTarget(row)}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          )}
                        </>
                      )}
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      <RuleSetEditDialog
        open={editOpen}
        initial={editing?.view ?? null}
        outboundOptions={outboundOptions}
        showScopePicker={!editing}
        initialScope="per_config"
        onCancel={() => setEditOpen(false)}
        onSubmit={handleSubmit}
      />

      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete local rule_set "{deleteTarget?.view.tag}"?</AlertDialogTitle>
            <AlertDialogDescription>
              Routing rules that reference{' '}
              <code className="text-xs">{deleteTarget?.view.tag}</code> in a{' '}
              <code className="text-xs">rule_set</code> matcher will fail validation after
              restart.
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
