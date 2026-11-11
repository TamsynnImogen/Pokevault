/** PVF1 is the desktop worker's little-endian RGBA8 + stereo PCM16 packet. */
export type EmulatorPacket = {
  width: number;
  height: number;
  sampleRate: number;
  sequence: number;
  pixels: Uint8ClampedArray<ArrayBuffer>;
  left: Float32Array<ArrayBuffer>;
  right: Float32Array<ArrayBuffer>;
};

export function parseEmulatorPacket(buffer: ArrayBuffer): EmulatorPacket {
  if (!(buffer instanceof ArrayBuffer) || buffer.byteLength < 28)
    throw new Error('Invalid emulator frame header.');
  const view = new DataView(buffer);
  if (
    view.getUint8(0) !== 80 ||
    view.getUint8(1) !== 86 ||
    view.getUint8(2) !== 70 ||
    view.getUint8(3) !== 49
  )
    throw new Error('Unknown emulator frame format.');
  const width = view.getUint32(4, true);
  const height = view.getUint32(8, true);
  const sampleRate = view.getUint32(12, true);
  const frames = view.getUint32(16, true);
  const sequence = view.getBigUint64(20, true);
  if (
    !width ||
    !height ||
    width > 1024 ||
    height > 1024 ||
    frames > 8192 ||
    sampleRate < 8000 ||
    sampleRate > 192000 ||
    sequence > BigInt(Number.MAX_SAFE_INTEGER)
  )
    throw new Error('Emulator frame exceeds supported bounds.');
  const pixelBytes = width * height * 4;
  if (buffer.byteLength !== 28 + pixelBytes + frames * 4)
    throw new Error('Emulator frame length mismatch.');
  const pixels = new Uint8ClampedArray(buffer, 28, pixelBytes);
  const left = new Float32Array(frames);
  const right = new Float32Array(frames);
  const audioOffset = 28 + pixelBytes;
  for (let i = 0; i < frames; i++) {
    left[i] = view.getInt16(audioOffset + i * 4, true) / 32768;
    right[i] = view.getInt16(audioOffset + i * 4 + 2, true) / 32768;
  }
  return { width, height, sampleRate, sequence: Number(sequence), pixels, left, right };
}
