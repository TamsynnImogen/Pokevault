// @vitest-environment jsdom
import { expect, test, vi } from 'vitest';
import { EmulatorPage } from './emulator';

const mocks = vi.hoisted(() => ({
  request: vi.fn(),
  start: vi.fn(),
  release: vi.fn(),
}));
vi.mock('./transport', () => ({
  request: mocks.request,
  startEmulator: mocks.start,
  releaseEmulatorChannel: mocks.release,
  chooseFile: vi.fn(),
}));

function frame(sequence: bigint): ArrayBuffer {
  const buffer = new ArrayBuffer(32);
  const data = new DataView(buffer);
  for (const [index, byte] of [80, 86, 70, 49].entries()) data.setUint8(index, byte);
  data.setUint32(4, 1, true);
  data.setUint32(8, 1, true);
  data.setUint32(12, 32768, true);
  data.setUint32(16, 0, true);
  data.setBigUint64(20, sequence, true);
  return buffer;
}

test('new session accepts and acknowledges an early sequence after a previous high sequence', async () => {
  mocks.request.mockReset();
  mocks.start.mockReset();
  mocks.request.mockResolvedValue({
    active: true,
    session_id: 'new',
    save_id: 3,
    state: 'running',
  });
  mocks.start.mockImplementation(
    async (_saveId: number, _corePath: string, onPacket: (packet: ArrayBuffer) => void) => {
      onPacket(frame(1n)); // Channel may deliver before emulator_start resolves.
      return { active: true, session_id: 'new', save_id: 3, state: 'running' };
    },
  );
  const refreshViews = vi.fn(async () => {});
  const page = new EmulatorPage(refreshViews);
  const internal = page as unknown as {
    overview: unknown;
    saveId: number;
    corePath: string;
    ack: number;
    start: () => Promise<void>;
  };
  internal.overview = {
    games: [{ hash: 'rom' }],
    saves: [{ id: 3, game_hash: 'rom', linked_path: '/tmp/play.sav', sync_status: 'linked' }],
  };
  internal.saveId = 3;
  internal.corePath = '/tmp/core.so';
  internal.ack = 900;
  await internal.start();
  await vi.waitFor(() =>
    expect(mocks.request).toHaveBeenCalledWith('emulator_control', {
      session_id: 'new',
      buttons: 0,
      ack: 1,
      paused: true,
    }),
  );
  expect(internal.ack).toBe(1);
  expect(refreshViews).toHaveBeenCalledOnce();
});

test('pending session releases its lease through recovery-copy action', async () => {
  mocks.request.mockReset();
  mocks.release.mockReset();
  const stopped = { active: false, session_id: null, save_id: null, state: 'stopped' };
  mocks.request.mockResolvedValue(stopped);
  const afterStop = vi.fn(async () => {});
  const page = new EmulatorPage(afterStop);
  const internal = page as unknown as {
    status: unknown;
    discard: () => Promise<void>;
  };
  internal.status = { active: true, session_id: 'pending-7', save_id: 3, state: 'save_pending' };
  await internal.discard();
  expect(mocks.request).toHaveBeenCalledWith('emulator_discard', { session_id: 'pending-7' });
  expect(mocks.release).toHaveBeenCalledOnce();
  expect(afterStop).toHaveBeenCalledOnce();
  expect(internal.status).toEqual(stopped);
});

test('active file label follows the native session save ID despite another global selection', () => {
  const page = new EmulatorPage(async () => {});
  const internal = page as unknown as {
    overview: unknown;
    saveId: number;
    status: unknown;
    update: () => void;
  };
  internal.overview = {
    saves: [
      { id: 1, linked_path: '/tmp/old.sav', sync_status: 'linked' },
      { id: 7, linked_path: '/tmp/playing.sav', sync_status: 'emulating' },
    ],
  };
  internal.saveId = 1;
  internal.status = { active: true, session_id: 'live', save_id: 7, state: 'running' };
  internal.update();
  expect(page.element.textContent).toContain('Active save: playing.sav');
  expect(page.element.textContent).not.toContain('old.sav');
});
