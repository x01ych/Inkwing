import { useEffect, useRef } from 'react';
import type { UnlistenFn } from '@tauri-apps/api/event';

/** Subscribe to a Tauri event for the lifetime of a component. Handles:
 *
 * - the React 18/19 StrictMode double-effect race (cleanup runs *before*
 *   the `listen()` promise resolves);
 * - `window.__TAURI_INTERNALS__` being undefined — happens when the dev
 *   page is opened in a regular browser instead of the Tauri webview, or
 *   on a Tauri version mismatch. Without this guard, listen() throws
 *   synchronously, the effect rejects, and React 19's stricter
 *   error-handling unmounts the entire tree → blank page.
 */
export function useEventListener(subscribe: () => Promise<UnlistenFn>, deps: unknown[] = []) {
  const subRef = useRef(subscribe);
  subRef.current = subscribe;

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;

    // Wrap in a fresh async fn so a synchronous throw inside listen()
    // (transformCallback on undefined __TAURI_INTERNALS__) becomes a
    // promise rejection we can swallow.
    (async () => {
      try {
        const un = await subRef.current();
        if (cancelled) un();
        else unlisten = un;
      } catch (e) {
        // Tauri IPC not available — log and continue rendering. The
        // event subscription is best-effort UI plumbing, never critical.
        console.warn('[useEventListener] subscribe failed:', e);
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
