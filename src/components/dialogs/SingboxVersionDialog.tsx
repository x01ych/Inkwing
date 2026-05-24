import { Download, Loader2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { singboxVersionsApi, type ReleaseAsset } from '../../api/core';
import { Alert, AlertDescription, AlertTitle } from '../ui/alert';
import { Badge } from '../ui/badge';
import { Button } from '../ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { ScrollArea } from '../ui/scroll-area';

interface Props {
  open: boolean;
  onClose: () => void;
  onDownloaded: () => void;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
}

export default function SingboxVersionDialog({ open, onClose, onDownloaded }: Props) {
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState('');
  const [releases, setReleases] = useState<ReleaseAsset[]>([]);
  const [downloadingVersion, setDownloadingVersion] = useState('');

  async function load() {
    setLoading(true);
    setErr('');
    try {
      setReleases(await singboxVersionsApi.listRemote());
    } catch (e) {
      setErr(String((e as Error)?.message ?? e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (open) load();
  }, [open]);

  async function handleInstall(rel: ReleaseAsset) {
    setDownloadingVersion(rel.version);
    setErr('');
    try {
      await singboxVersionsApi.download(rel.version, rel.asset_url);
      toast.success(`Installed sing-box ${rel.version}`);
      onDownloaded();
      // Re-fetch so the "installed" badge updates.
      await load();
    } catch (e) {
      const msg = String((e as Error)?.message ?? e);
      setErr(msg);
      toast.error(`Install failed: ${msg}`);
    } finally {
      setDownloadingVersion('');
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Download sing-box version</DialogTitle>
          <DialogDescription>
            Fetched live from GitHub releases. Newer versions are first. Pick one and click
            Install — the binary is unpacked into Inkwing's data directory and can then be
            selected on the Dashboard.
          </DialogDescription>
        </DialogHeader>

        {err && (
          <Alert variant="destructive">
            <AlertTitle>Could not load releases</AlertTitle>
            <AlertDescription>{err}</AlertDescription>
          </Alert>
        )}

        <ScrollArea className="h-[400px] rounded-md border">
          {loading ? (
            <div className="flex h-full items-center justify-center p-8 text-sm text-muted-foreground">
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              Loading…
            </div>
          ) : releases.length === 0 ? (
            <div className="p-8 text-center text-sm text-muted-foreground">
              No releases found for your platform.
            </div>
          ) : (
            <ul className="divide-y">
              {releases.map((r) => (
                <li
                  key={r.version}
                  className="flex items-center justify-between gap-3 px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-sm font-medium">{r.version}</span>
                      {r.prerelease && (
                        <Badge variant="outline" className="text-[10px]">
                          prerelease
                        </Badge>
                      )}
                      {r.installed && (
                        <Badge variant="secondary" className="text-[10px]">
                          installed
                        </Badge>
                      )}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {r.asset_name} · {formatSize(r.size)}
                      {r.published_at && ` · ${r.published_at.slice(0, 10)}`}
                    </div>
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!!downloadingVersion || r.installed}
                    onClick={() => handleInstall(r)}
                  >
                    {downloadingVersion === r.version ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Download className="h-4 w-4" />
                    )}
                    {r.installed ? 'Installed' : 'Install'}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </ScrollArea>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
