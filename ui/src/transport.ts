import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Channel } from '@tauri-apps/api/core';
import type { EmulatorStatus } from './types';

/** The product uses one native command. A plain browser has no collection access. */
export async function request<T>(action: string, fields: Record<string, unknown> = {}): Promise<T> {
  if (!('__TAURI_INTERNALS__' in window))
    throw new Error(
      'Open the installed desktop app to access your local vault. The browser preview cannot connect to native storage.',
    );
  return invoke<T>('vault_request', { request: { action, ...fields } });
}

export async function chooseFile(extensions: string[]): Promise<string | null> {
  if (!('__TAURI_INTERNALS__' in window))
    throw new Error('File selection is available in the desktop app.');
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [{ name: 'Supported files', extensions }],
  });
  return typeof selected === 'string' ? selected : null;
}

/** Keep the channel referenced for the lifetime of the native session. */
let frameChannel: Channel<ArrayBuffer> | null = null;
export async function startEmulator(
  save_id: number,
  core_path: string,
  onPacket: (packet: ArrayBuffer) => void,
): Promise<EmulatorStatus> {
  if (!('__TAURI_INTERNALS__' in window))
    throw new Error('Emulation is available in the desktop app.');
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = onPacket;
  frameChannel = channel;
  try {
    const status = await invoke<EmulatorStatus>('emulator_start', {
      save_id,
      core_path,
      on_frame: channel,
    });
    if (!status.active) frameChannel = null;
    return status;
  } catch (error) {
    frameChannel = null;
    throw error;
  }
}

export function releaseEmulatorChannel(): void {
  frameChannel = null;
}
