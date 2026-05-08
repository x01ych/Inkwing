import { invoke as rawInvoke } from '@tauri-apps/api/core';

/** Thin typed wrapper. Backend Tauri commands return Result<T, AppError>;
 * AppError is serialized as a string by error.rs. We rethrow the string. */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await rawInvoke<T>(cmd, args ?? {});
  } catch (e) {
    if (typeof e === 'string') throw new Error(e);
    throw e;
  }
}
