import { expect, test } from 'vitest';
import { parseEmulatorPacket } from './emulatorPacket';

function packet(): ArrayBuffer {
  const buffer = new ArrayBuffer(28 + 2 * 4 + 2 * 4);
  const view = new DataView(buffer);
  for (const [index, byte] of [80, 86, 70, 49].entries()) view.setUint8(index, byte);
  view.setUint32(4, 2, true);
  view.setUint32(8, 1, true);
  view.setUint32(12, 32768, true);
  view.setUint32(16, 2, true);
  view.setBigUint64(20, 42n, true);
  new Uint8Array(buffer, 28, 8).set([255, 0, 0, 255, 0, 0, 255, 255]);
  view.setInt16(36, -32768, true);
  view.setInt16(38, 16384, true);
  view.setInt16(40, 0, true);
  view.setInt16(42, -16384, true);
  return buffer;
}

test('decodes exact PVF1 RGBA and interleaved stereo PCM', () => {
  const frame = parseEmulatorPacket(packet());
  expect([frame.width, frame.height, frame.sampleRate, frame.sequence]).toEqual([2, 1, 32768, 42]);
  expect([...frame.pixels]).toEqual([255, 0, 0, 255, 0, 0, 255, 255]);
  expect([...frame.left]).toEqual([-1, 0]);
  expect([...frame.right]).toEqual([0.5, -0.5]);
});

test('rejects malformed, oversized, and unsafe-sequence packets before allocation', () => {
  const cases = [packet().slice(0, 27), packet().slice(0, 43), packet(), packet(), packet()];
  new DataView(cases[2]).setUint32(4, 10000, true);
  new DataView(cases[3]).setUint32(16, 1000000, true);
  new DataView(cases[4]).setBigUint64(20, BigInt(Number.MAX_SAFE_INTEGER) + 1n, true);
  for (const candidate of cases) expect(() => parseEmulatorPacket(candidate)).toThrow();
});
