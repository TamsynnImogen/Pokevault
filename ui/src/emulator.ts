import { chooseFile, releaseEmulatorChannel, request, startEmulator } from './transport';
import { parseEmulatorPacket, type EmulatorPacket } from './emulatorPacket';
import type { EmulatorStatus, Overview, SaveSummary } from './types';
import { listen } from '@tauri-apps/api/event';

const keys: Record<string, number> = {
  KeyZ: 0,
  ShiftLeft: 2,
  ShiftRight: 2,
  Enter: 3,
  ArrowUp: 4,
  ArrowDown: 5,
  ArrowLeft: 6,
  ArrowRight: 7,
  KeyX: 8,
  KeyA: 10,
  KeyS: 11,
};
const controls: [string, number][] = [
  ['↑', 4],
  ['←', 6],
  ['↓', 5],
  ['→', 7],
  ['B', 0],
  ['A', 8],
  ['L', 10],
  ['R', 11],
  ['Select', 2],
  ['Start', 3],
];

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = '',
  text = '',
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = text;
  return element;
}
function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** One long-lived view and bounded control pump; main.render may move its root. */
export class EmulatorPage {
  readonly element = node('section', 'emulator-page');
  private readonly canvas = node('canvas', 'emulator-canvas');
  private readonly statusLine = node('p', 'meta');
  private readonly errorLine = node('p', 'banner error');
  private readonly saveLine = node('p', 'muted');
  private readonly coreInput = node('input');
  private readonly startButton = node('button', 'primary', 'Start game');
  private readonly pauseButton = node('button', 'secondary', 'Pause');
  private readonly stopButton = node('button', 'secondary', 'Stop & save');
  private readonly discardButton = node(
    'button',
    'danger-outline',
    'Keep recovery copy & close session',
  );
  private readonly pendingNote = node('p', 'banner error');
  private readonly soundButton = node('button', 'quiet', 'Enable audio');
  private overview: Overview | null = null;
  private saveId: number | null = null;
  private status: EmulatorStatus | null = null;
  private corePath = '';
  private visible = false;
  private loaded = false;
  private busy = false;
  private error = '';
  private keysDown = new Set<string>();
  private pointers = new Map<number, number>();
  private buttons = 0;
  private ack = -1;
  private sentAck = -1;
  private sentButtons = 0;
  private desiredPaused = false;
  private sentPaused = false;
  private controlBusy = false;
  private stopping = false;
  private lostFocus = false;
  private generation = 0;
  private audio: AudioContext | null = null;
  private audioSources = new Set<AudioBufferSourceNode>();
  private nextAudioTime = 0;
  private readonly afterStop: () => Promise<void>;

  constructor(afterStop: () => Promise<void>) {
    this.afterStop = afterStop;
    const intro = node(
      'p',
      'muted',
      'Play the ROM linked to the selected managed save with a local mGBA Libretro core. Save normally in-game, then choose Stop & save before changing Pokémon in the Vault.',
    );
    const setup = node('div', 'emulator-setup');
    const pathLabel = node('label', 'field', 'mGBA Libretro core (.so, .dll, .dylib)');
    this.coreInput.type = 'text';
    this.coreInput.placeholder = 'Choose a local mGBA core';
    this.coreInput.addEventListener('input', () => {
      this.corePath = this.coreInput.value;
      this.update();
    });
    pathLabel.append(this.coreInput);
    const browse = node('button', 'quiet', 'Choose mGBA core…');
    browse.type = 'button';
    browse.addEventListener('click', () => void this.chooseCore());
    setup.append(pathLabel, browse);
    this.canvas.width = 240;
    this.canvas.height = 160;
    this.canvas.tabIndex = 0;
    this.canvas.setAttribute('aria-label', 'Game display; click to use keyboard controls');
    this.canvas.addEventListener('blur', () => this.clearInput());
    const actions = node('div', 'emulator-actions');
    for (const button of [
      this.startButton,
      this.pauseButton,
      this.stopButton,
      this.discardButton,
      this.soundButton,
    ])
      button.type = 'button';
    this.startButton.addEventListener('click', () => void this.start());
    this.pauseButton.addEventListener('click', () => void this.togglePause());
    this.stopButton.addEventListener('click', () => void this.stop());
    this.discardButton.addEventListener('click', () => void this.discard());
    this.soundButton.addEventListener('click', () => void this.enableAudio());
    actions.append(
      this.startButton,
      this.pauseButton,
      this.stopButton,
      this.discardButton,
      this.soundButton,
    );
    const pad = node('div', 'emulator-pad');
    for (const [label, bit] of controls) {
      const button = node('button', 'quiet', label);
      button.type = 'button';
      button.setAttribute('aria-label', `${label} game button`);
      button.addEventListener('pointerdown', (event) => {
        if (!this.isRunning()) return;
        event.preventDefault();
        button.setPointerCapture(event.pointerId);
        this.pointers.set(event.pointerId, bit);
        this.recomputeButtons();
      });
      const release = (event: PointerEvent) => {
        this.pointers.delete(event.pointerId);
        this.recomputeButtons();
      };
      button.addEventListener('pointerup', release);
      button.addEventListener('pointercancel', release);
      button.addEventListener('lostpointercapture', release);
      pad.append(button);
    }
    const help = node(
      'p',
      'muted small',
      'Click the display for keyboard input: arrows move, Z = B, X = A, Enter = Start, Shift = Select, A = L, S = R. On-screen buttons also work. Standard in-game saves are used; savestates are not synchronized with Vault edits.',
    );
    this.element.append(
      node('h2', '', 'Play game'),
      intro,
      this.saveLine,
      setup,
      this.statusLine,
      this.errorLine,
      this.pendingNote,
      this.canvas,
      actions,
      pad,
      help,
    );
    document.addEventListener('keydown', (event) => this.onKey(event, true));
    document.addEventListener('keyup', (event) => this.onKey(event, false));
    window.addEventListener('blur', () => this.pauseForLoss());
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) this.pauseForLoss();
    });
    window.addEventListener('pagehide', () => this.pauseForLoss());
    if ('__TAURI_INTERNALS__' in window)
      void listen<string>('emulator-close-error', (event) => {
        this.error = event.payload;
        void this.readStatus();
        this.update();
      }).catch((error) => {
        this.error = message(error);
        this.update();
      });
    this.update();
  }

  mount(parent: HTMLElement, overview: Overview, saveId: number | null): void {
    this.visible = true;
    this.overview = overview;
    this.saveId = saveId;
    parent.append(this.element);
    if (!this.loaded) {
      this.loaded = true;
      void this.loadSession();
    }
    this.update();
  }
  hide(): void {
    if (!this.visible) return;
    this.visible = false;
    this.pauseForLoss();
  }
  private async loadSession(): Promise<void> {
    try {
      const [defaults, status] = await Promise.all([
        request<{ core_path: string | null }>('emulator_defaults'),
        request<EmulatorStatus>('emulator_status'),
      ]);
      this.corePath ||= defaults.core_path ?? '';
      this.coreInput.value = this.corePath;
      this.status = status;
      this.desiredPaused = status.state === 'paused';
      this.sentPaused = this.desiredPaused;
      if (status.active && status.state === 'running') this.pauseForLoss();
    } catch (error) {
      this.error = message(error);
    }
    this.update();
  }
  private async readStatus(): Promise<void> {
    try {
      this.status = await request<EmulatorStatus>('emulator_status');
    } catch (error) {
      this.error = message(error);
    }
    this.update();
  }
  private selectedSave(): SaveSummary | undefined {
    return this.overview?.saves.find((save) => save.id === this.saveId);
  }
  private ready(): boolean {
    const save = this.selectedSave();
    return (
      !!save?.linked_path &&
      save.sync_status === 'linked' &&
      !!this.overview?.games.some((game) => game.hash === save.game_hash)
    );
  }
  private isRunning(): boolean {
    return !!this.status?.active && !this.desiredPaused && this.status.state === 'running';
  }
  private async chooseCore(): Promise<void> {
    try {
      const selected = await chooseFile(['so', 'dll', 'dylib']);
      if (selected) {
        this.corePath = selected;
        this.coreInput.value = selected;
        this.update();
      }
    } catch (error) {
      this.error = message(error);
      this.update();
    }
  }
  private async enableAudio(): Promise<void> {
    try {
      this.audio ??= new AudioContext();
      await this.audio.resume();
      this.nextAudioTime = this.audio.currentTime;
    } catch (error) {
      this.error = message(error);
    }
    this.update();
  }
  private async start(): Promise<void> {
    if (this.busy || !this.ready() || this.saveId == null || !this.corePath.trim()) return;
    this.busy = true;
    this.error = '';
    this.lostFocus = false;
    this.clearInput();
    this.buttons = 0;
    this.ack = -1;
    this.sentAck = -1;
    this.sentButtons = 0;
    this.desiredPaused = false;
    this.sentPaused = false;
    const generation = ++this.generation;
    // Construct and resume from this genuine click before the native await.
    void this.enableAudio();
    this.update();
    try {
      const status = await startEmulator(this.saveId, this.corePath.trim(), (packet) => {
        if (this.generation === generation) this.onPacket(packet);
      });
      this.status = status;
      this.desiredPaused = this.lostFocus || !this.visible || document.hidden;
      this.pumpControl();
      await this.afterStop();
    } catch (error) {
      this.error = message(error);
    } finally {
      this.busy = false;
      this.update();
    }
  }
  private async togglePause(): Promise<void> {
    if (!this.status?.active || this.busy) return;
    if (this.desiredPaused) {
      await this.enableAudio();
      this.lostFocus = false;
      this.desiredPaused = false;
    } else {
      this.desiredPaused = true;
      this.clearInput();
      this.stopAudio();
    }
    this.pumpControl();
    this.update();
  }
  private async stop(): Promise<void> {
    const session_id = this.status?.session_id;
    if (!session_id || this.busy) return;
    this.busy = true;
    this.stopping = true;
    this.clearInput();
    this.stopAudio();
    this.update();
    try {
      this.status = await request<EmulatorStatus>('emulator_stop', { session_id });
      if (this.status.state === 'stopped') releaseEmulatorChannel();
      await this.afterStop();
    } catch (error) {
      const stopError = message(error);
      await this.readStatus();
      this.error = stopError;
      if (this.status?.state === 'save_pending') {
        try {
          await this.afterStop();
        } catch {
          /* Keep the native save error visible. */
        }
      }
    } finally {
      this.busy = false;
      this.stopping = false;
      this.update();
    }
  }
  private async discard(): Promise<void> {
    const session_id = this.status?.session_id;
    if (!session_id || this.status?.state !== 'save_pending' || this.busy) return;
    this.busy = true;
    this.stopping = true;
    this.error = '';
    this.update();
    try {
      this.status = await request<EmulatorStatus>('emulator_discard', { session_id });
      releaseEmulatorChannel();
      await this.afterStop();
    } catch (error) {
      const discardError = message(error);
      await this.readStatus();
      this.error = discardError;
    } finally {
      this.busy = false;
      this.stopping = false;
      this.update();
    }
  }
  private pauseForLoss(): void {
    this.lostFocus = true;
    this.clearInput();
    this.stopAudio();
    if (this.status?.active) {
      this.desiredPaused = true;
      this.pumpControl();
    }
    this.update();
  }
  private onKey(event: KeyboardEvent, down: boolean): void {
    const bit = keys[event.code];
    if (bit === undefined) return;
    if (down && (!this.isRunning() || document.activeElement !== this.canvas)) return;
    if (down) {
      event.preventDefault();
      this.keysDown.add(event.code);
    } else this.keysDown.delete(event.code);
    this.recomputeButtons();
  }
  private clearInput(): void {
    this.keysDown.clear();
    this.pointers.clear();
    this.recomputeButtons();
  }
  private recomputeButtons(): void {
    let buttons = 0;
    for (const key of this.keysDown) buttons |= 1 << keys[key];
    for (const bit of this.pointers.values()) buttons |= 1 << bit;
    if (buttons === this.buttons) return;
    this.buttons = buttons;
    this.pumpControl();
  }
  private onPacket(buffer: ArrayBuffer): void {
    try {
      const packet = parseEmulatorPacket(buffer);
      if (!this.status?.active) {
        this.ack = Math.max(this.ack, packet.sequence);
        return;
      }
      if (packet.sequence <= this.ack) return;
      this.ack = packet.sequence;
      if (this.visible && !this.desiredPaused) {
        this.draw(packet);
        this.playAudio(packet);
      }
      this.pumpControl();
    } catch (error) {
      this.error = message(error);
      this.update();
    }
  }
  private draw(packet: EmulatorPacket): void {
    const context = this.canvas.getContext('2d');
    if (!context) return;
    if (this.canvas.width !== packet.width || this.canvas.height !== packet.height) {
      this.canvas.width = packet.width;
      this.canvas.height = packet.height;
    }
    context.putImageData(new ImageData(packet.pixels, packet.width, packet.height), 0, 0);
  }
  private playAudio(packet: EmulatorPacket): void {
    const audio = this.audio;
    if (!audio || audio.state !== 'running' || !packet.left.length) return;
    if (this.nextAudioTime - audio.currentTime > 0.15) return; // Drop stale audio instead of growing latency.
    const buffer = audio.createBuffer(2, packet.left.length, packet.sampleRate);
    buffer.copyToChannel(packet.left, 0);
    buffer.copyToChannel(packet.right, 1);
    const source = audio.createBufferSource();
    source.buffer = buffer;
    source.connect(audio.destination);
    source.onended = () => this.audioSources.delete(source);
    source.start(Math.max(audio.currentTime, this.nextAudioTime));
    this.audioSources.add(source);
    this.nextAudioTime = Math.max(audio.currentTime, this.nextAudioTime) + buffer.duration;
  }
  private stopAudio(): void {
    for (const source of this.audioSources) {
      try {
        source.stop();
      } catch {
        /* Already ended. */
      }
    }
    this.audioSources.clear();
    if (this.audio) this.nextAudioTime = this.audio.currentTime;
  }
  /** At most one IPC request is in flight; only the latest input and ack survive. */
  private pumpControl(): void {
    const session_id = this.status?.session_id;
    if (
      !session_id ||
      !this.status?.active ||
      !['running', 'paused', 'starting'].includes(this.status.state) ||
      this.controlBusy ||
      this.stopping
    )
      return;
    if (
      this.ack === this.sentAck &&
      this.buttons === this.sentButtons &&
      this.desiredPaused === this.sentPaused
    )
      return;
    const ack = this.ack;
    const buttons = this.buttons;
    const paused = this.desiredPaused;
    this.controlBusy = true;
    let failed = false;
    void request<EmulatorStatus>('emulator_control', {
      session_id,
      buttons,
      ...(ack >= 0 ? { ack } : {}),
      paused,
    })
      .then((status) => {
        if (this.status?.session_id !== session_id || this.stopping) return;
        this.status = status;
        this.sentAck = ack;
        this.sentButtons = buttons;
        this.sentPaused = paused;
        this.update();
      })
      .catch((error) => {
        failed = true;
        this.error = message(error);
        this.update();
      })
      .finally(() => {
        this.controlBusy = false;
        if (!failed) this.pumpControl();
      });
  }
  private update(): void {
    const status = this.status;
    const save = status?.active
      ? this.overview?.saves.find((item) => item.id === status.save_id)
      : this.selectedSave();
    this.saveLine.textContent = save?.linked_path
      ? `${status?.active ? 'Active' : 'Selected'} save: ${save.linked_path.split(/[\\/]/).pop()} · ${save.sync_status ?? 'unknown'}`
      : status?.active
        ? `Active save ${status.save_id ?? 'unknown'} is unavailable in the current overview.`
        : 'Choose a linked managed save in the game library first.';
    this.statusLine.textContent = status?.active
      ? `${status.state} · ${status.core_name ?? 'Libretro core'}${status.fps ? ` · ${status.fps.toFixed(1)} fps` : ''}${status.frames != null ? ` · ${status.frames} frames` : ''}`
      : status?.state === 'save_pending'
        ? 'Save pending: inspect the selected save before Vault transfers.'
        : 'No game running.';
    this.errorLine.textContent = this.error || status?.error || '';
    this.errorLine.hidden = !this.errorLine.textContent;
    this.pendingNote.textContent =
      status?.state === 'save_pending'
        ? 'The game has stopped, but its save could not be safely written to the selected file. Stop & save retries. Keep recovery copy & close session retains the latest emulated save in the app workspace and releases the session; it does not overwrite the selected disk file. A linked-file journal may still need recovery before Vault transfers.'
        : '';
    this.pendingNote.hidden = !this.pendingNote.textContent;
    this.startButton.disabled =
      this.busy || !!status?.active || !this.ready() || !this.corePath.trim();
    this.pauseButton.disabled =
      this.busy || !status?.active || !['running', 'paused'].includes(status.state);
    this.pauseButton.textContent = this.desiredPaused ? 'Resume' : 'Pause';
    this.stopButton.disabled = this.busy || !status?.active;
    this.discardButton.hidden = status?.state !== 'save_pending';
    this.discardButton.disabled = this.busy || status?.state !== 'save_pending';
    this.soundButton.textContent =
      this.audio?.state === 'running' ? 'Audio enabled' : 'Enable audio';
    this.soundButton.disabled = this.audio?.state === 'running';
  }
}
