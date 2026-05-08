import {
  ArrowLeft,
  CheckCircle,
  FolderOpen,
  Loader2,
} from 'lucide-react';
import Editor from '@monaco-editor/react';
import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { toast } from 'sonner';
import {
  configApi,
  type ConfigEntrySummary,
  type ValidationReport,
} from '../api/config';
import { Alert, AlertDescription, AlertTitle } from '../components/ui/alert';
import { Badge } from '../components/ui/badge';
import { Button } from '../components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '../components/ui/card';

export default function ConfigDetailPage() {
  const { id = '' } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [entry, setEntry] = useState<ConfigEntrySummary | null>(null);
  const [raw, setRaw] = useState('');
  const [validation, setValidation] = useState<ValidationReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [validating, setValidating] = useState(false);
  const [err, setErr] = useState('');

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    Promise.all([configApi.libraryList(), configApi.libraryView(id)])
      .then(([libs, text]) => {
        if (cancelled) return;
        setEntry(libs.find((e) => e.id === id) ?? null);
        setRaw(text);
      })
      .catch((e) => {
        if (cancelled) return;
        setErr(String((e as Error)?.message ?? e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  async function handleValidate() {
    if (!entry) return;
    setValidating(true);
    try {
      const r = await configApi.validate(entry.storage_path);
      setValidation(r);
      if (r.ok) toast.success('sing-box check passed');
    } catch (e) {
      toast.error(String((e as Error)?.message ?? e));
    } finally {
      setValidating(false);
    }
  }

  if (loading) {
    return (
      <div className="flex h-32 items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }
  if (err) {
    return (
      <Alert variant="destructive">
        <AlertTitle>Could not load entry</AlertTitle>
        <AlertDescription>{err}</AlertDescription>
      </Alert>
    );
  }
  if (!entry) {
    return (
      <div className="flex h-32 items-center justify-center text-sm text-muted-foreground">
        Entry not found in library
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Link to="/config">
            <Button variant="outline" size="sm">
              <ArrowLeft className="h-4 w-4" />
              Back
            </Button>
          </Link>
          <h2 className="text-2xl font-semibold tracking-tight">{entry.name}</h2>
          {entry.is_active && <Badge>active</Badge>}
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={validating}
            onClick={handleValidate}
          >
            {validating ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <CheckCircle className="h-4 w-4" />
            )}
            Validate (sing-box check)
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => configApi.libraryReveal(entry.id)}
          >
            <FolderOpen className="h-4 w-4" />
            Reveal in folder
          </Button>
          {!entry.is_active && (
            <Button
              size="sm"
              onClick={async () => {
                try {
                  await configApi.librarySelect(entry.id);
                  toast.success('Set as active');
                  navigate('/config');
                } catch (e) {
                  toast.error(String((e as Error)?.message ?? e));
                }
              }}
            >
              Use this
            </Button>
          )}
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Summary</CardTitle>
        </CardHeader>
        <CardContent>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-sm">
            <KV
              label="Source"
              span
              value={
                entry.source.kind === 'subscription' ? (
                  <Badge variant="secondary">
                    subscription · {entry.source.sub_id.slice(0, 8)}…
                  </Badge>
                ) : (
                  <Badge variant="outline">
                    local{entry.source.original_path ? ` · ${entry.source.original_path}` : ''}
                  </Badge>
                )
              }
            />
            <KV
              label="Storage path"
              span
              value={<code className="text-xs">{entry.storage_path}</code>}
            />
            <KV label="Outbounds" value={entry.outbound_count ?? '—'} />
            <KV label="Rules" value={entry.rule_count ?? '—'} />
            <KV
              label="Has TUN inbound"
              value={
                entry.has_tun_inbound ? (
                  <Badge variant="default">yes</Badge>
                ) : (
                  <Badge variant="outline">no</Badge>
                )
              }
            />
          </dl>
        </CardContent>
      </Card>

      {validation && !validation.ok && (
        <Alert variant="destructive">
          <AlertTitle>sing-box check failed (exit {validation.exit_code ?? '?'})</AlertTitle>
          <AlertDescription>
            <ul className="ml-4 list-disc space-y-1">
              {validation.errors.map((er, idx) => (
                <li key={idx}>
                  <Badge variant="destructive" className="mr-1">
                    {er.level}
                  </Badge>
                  {er.message}
                </li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      )}
      {validation?.ok && (
        <Alert>
          <AlertTitle>sing-box check passed</AlertTitle>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Raw JSON (read-only)</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <Editor
            height="60vh"
            language="json"
            theme="vs-dark"
            value={raw}
            options={{
              readOnly: true,
              minimap: { enabled: false },
              fontSize: 13,
              wordWrap: 'on',
            }}
          />
        </CardContent>
      </Card>
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
