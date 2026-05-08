import { Minus, Square, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { type as osType } from '@tauri-apps/plugin-os';
import { cn } from '../../lib/utils';

/** Inline window controls used in the page header now that we've dropped
 * the dedicated title bar. macOS renders nothing — the OS overlays its
 * native traffic-light controls (titleBarStyle: "Overlay" in
 * tauri.conf.json reserves ~80 px of header space on the left). Win/Linux
 * gets three flat buttons on the right. */
export default function WindowControls() {
  const [platform, setPlatform] = useState<string>('');
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        // osType() is sync in @tauri-apps/plugin-os v2.
        const p = osType();
        if (cancelled) return;
        setPlatform(p);
        const w = getCurrentWindow();
        setMaximized(await w.isMaximized());
        const un = await w.onResized(async () => {
          if (cancelled) return;
          setMaximized(await w.isMaximized());
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        // Outside Tauri webview — no controls.
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  if (platform === 'macos') return null;

  return (
    <div className="flex">
      <Btn onClick={() => getCurrentWindow().minimize()} aria-label="Minimize">
        <Minus size={14} />
      </Btn>
      <Btn
        onClick={async () => {
          const w = getCurrentWindow();
          if (await w.isMaximized()) await w.unmaximize();
          else await w.maximize();
        }}
        aria-label={maximized ? 'Restore' : 'Maximize'}
      >
        <Square size={11} strokeWidth={2.4} />
      </Btn>
      <Btn onClick={() => getCurrentWindow().close()} aria-label="Close" danger>
        <X size={14} />
      </Btn>
    </div>
  );
}

function Btn({
  onClick,
  children,
  danger,
  ...rest
}: {
  onClick: () => void;
  children: React.ReactNode;
  danger?: boolean;
  'aria-label': string;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        'flex h-10 w-11 items-center justify-center text-muted-foreground transition-colors',
        danger
          ? 'hover:bg-destructive hover:text-destructive-foreground'
          : 'hover:bg-sidebar-accent hover:text-foreground'
      )}
      {...rest}
    >
      {children}
    </button>
  );
}

/** Helper for the header that reserves traffic-light space on macOS only. */
export function HeaderMacInset() {
  const [isMac, setIsMac] = useState(false);
  useEffect(() => {
    try {
      setIsMac(osType() === 'macos');
    } catch {
      // not in Tauri webview
    }
  }, []);
  return isMac ? <div className="w-20 shrink-0" /> : null;
}
