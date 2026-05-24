import { invoke } from './tauri';

export interface CoreStatus {
  running: boolean;
  pid: number | null;
  version: string | null;
  started_at_ms: number | null;
  clash_api_addr: string | null;
  /** Bumped on each successful core_start. Frontend pump-event filters
   * use this to drop stragglers from a previous session. */
  epoch: number;
  recent_stderr: string[];
}

export interface PrivilegeReport {
  tun_capable: boolean;
  hint: string;
}

export const coreApi = {
  /** Synchronous one-shot: spawn `sing-box version`. */
  version: () => invoke<string>('core_version'),
  status: () => invoke<CoreStatus>('core_status'),
  start: () => invoke<CoreStatus>('core_start'),
  stop: () => invoke<void>('core_stop'),
  restart: () => invoke<CoreStatus>('core_restart'),
  checkPrivilege: () => invoke<PrivilegeReport>('core_check_privilege'),

  /** Linux: `pkexec setcap cap_net_admin,...` on the bundled sing-box.
   *  Persistent. Available only on Linux. */
  grantTunCapabilityLinux: () => invoke<void>('core_grant_tun_capability_linux'),
  /** Windows: persist tun_enabled=true, then PowerShell Start-Process
   *  -Verb RunAs on this exe and exit current process. UAC follows.
   *  Resolves via `app.exit(0)` so the promise typically never settles
   *  in the calling page — treat it as fire-and-forget. */
  relaunchAsAdminWindows: () => invoke<void>('core_relaunch_as_admin_windows'),
  /** macOS: probe Touch ID / password authorization by running an empty
   *  admin shell. Resolves on success, rejects on user cancel. */
  testMacosAdmin: () => invoke<void>('core_test_macos_admin'),
};

// ---- multi-version sing-box management ------------------------------

export interface InstalledBinary {
  /** "v1.10.7" or the literal "bundled" for the Tauri sidecar. */
  version: string;
  path: string;
  is_bundled: boolean;
}

export interface ReleaseAsset {
  version: string;
  asset_url: string;
  asset_name: string;
  size: number;
  published_at: string;
  prerelease: boolean;
  /** True if this version is already in `<data_dir>/binaries/`. */
  installed: boolean;
}

export interface ValidationError {
  level: string;
  message: string;
}

export interface ValidationReport {
  ok: boolean;
  exit_code: number | null;
  errors: ValidationError[];
}

export const singboxVersionsApi = {
  list: () => invoke<InstalledBinary[]>('singbox_versions_list'),
  listRemote: () => invoke<ReleaseAsset[]>('singbox_versions_list_remote'),
  download: (version: string, assetUrl: string) =>
    invoke<InstalledBinary>('singbox_versions_download', { version, assetUrl }),
  delete: (version: string) => invoke<void>('singbox_versions_delete', { version }),
  /** Validate the merged runtime config against the candidate binary;
   *  if it passes, persist + restart the core under that version. The
   *  returned report's `ok=false` means the switch did NOT happen. */
  select: (version: string) =>
    invoke<ValidationReport>('singbox_versions_select', { version }),
};

/** Detect host platform from the Tauri webview UA. Returns one of
 *  'windows' | 'macos' | 'linux' | 'other'. */
export function detectPlatform(): 'windows' | 'macos' | 'linux' | 'other' {
  if (typeof navigator === 'undefined') return 'other';
  const ua = navigator.userAgent;
  if (/Windows/i.test(ua)) return 'windows';
  if (/Mac OS X|Macintosh/i.test(ua)) return 'macos';
  if (/Linux/i.test(ua)) return 'linux';
  return 'other';
}

/** Shape a backend Err carries when settings_set rejects a TUN flip
 *  because the host is not capable. The frontend opens the privilege
 *  dialog instead of toasting. */
export interface PrivilegeRequiredError {
  kind: 'privilege_required';
  platform: string;
  hint: string;
}

export function isPrivilegeRequiredError(e: unknown): e is PrivilegeRequiredError {
  return (
    typeof e === 'object' &&
    e !== null &&
    (e as { kind?: unknown }).kind === 'privilege_required'
  );
}

export interface TrafficTick {
  up: number;
  down: number;
  ts_ms: number;
  epoch: number;
}
