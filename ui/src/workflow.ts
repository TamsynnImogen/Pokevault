import type { Operation, Pokemon, Preview } from './types';

/** Undo targets only the transfer that produced the currently viewed revision. */
export function canUndo(
  history: Operation[],
  saveId: number | null,
  revision: number | null,
): boolean {
  if (saveId == null || revision == null) return false;
  return history.some(
    (operation) =>
      operation.save_id === saveId &&
      operation.after_revision === revision &&
      !operation.undone &&
      ['copy-import', 'managed-deposit', 'withdraw'].includes(operation.kind),
  );
}

/** A preview belongs to one exact managed-save target and vault record. */
export type PreviewContext = { save_id: number; vault_id: number; box_index: number; slot: number };
export type BoundPreview = { context: PreviewContext; result: Preview };

export function sameContext(left: PreviewContext, right: PreviewContext): boolean {
  return (
    left.save_id === right.save_id &&
    left.vault_id === right.vault_id &&
    left.box_index === right.box_index &&
    left.slot === right.slot
  );
}

export function canApplyPreview(
  bound: BoundPreview | null,
  current: PreviewContext | null,
  currentRevision: number | null,
): boolean {
  return (
    !!bound &&
    !!current &&
    currentRevision === bound.result.revision &&
    sameContext(bound.context, current) &&
    bound.result.destination_slot_empty &&
    !hasEntries(bound.result.report.blocking_problems)
  );
}

function hasEntries(value: unknown): boolean {
  if (value == null) return false;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === 'object') return Object.keys(value).length > 0;
  return String(value).length > 0;
}

/** The IPC lists occupied records only. Expand them into visible fixed slots. */
export function visibleSlots(pokemon: Pokemon[], box: number, length: number): (Pokemon | null)[] {
  const slots: (Pokemon | null)[] = Array.from({ length }, () => null);
  for (const record of pokemon) {
    const location = record.location;
    const slot =
      box === -1 && location && 'Party' in location
        ? location.Party.slot
        : box >= 0 && location && 'Box' in location && location.Box.box_index === box
          ? location.Box.slot
          : -1;
    if (slot >= 0 && slot < length) slots[slot] = record;
  }
  return slots;
}

/** Build numbered slots as DOM nodes; callers supply only occupied-record controls. */
export function slotGrid(
  pokemon: Pokemon[],
  box: number,
  length: number,
  occupied: (record: Pokemon) => Node,
): HTMLDivElement {
  const grid = document.createElement('div');
  grid.className = box === -1 ? 'slot-grid party-grid' : 'slot-grid pc-grid';
  visibleSlots(pokemon, box, length).forEach((record, index) => {
    const cell = document.createElement('div');
    cell.className = `slot ${record ? 'occupied' : 'vacant'}`;
    const number = document.createElement('span');
    number.className = 'slot-number';
    number.textContent = String(index + 1);
    cell.append(number);
    if (record) cell.append(occupied(record));
    else {
      const empty = document.createElement('span');
      empty.className = 'slot-empty';
      empty.textContent = 'Empty';
      cell.append(empty);
    }
    grid.append(cell);
  });
  return grid;
}
