// @vitest-environment jsdom
import { expect, test } from 'vitest';
import { canApplyPreview, canUndo, slotGrid, visibleSlots, type BoundPreview } from './workflow';
import type { Pokemon } from './types';

const record = {
  location: { Box: { box_index: 0, slot: 7 } },
  nickname: '<img>',
  species_name: 'Species',
} as Pokemon;

test('PC grid renders all 30 numbered slots with only occupied records actionable', () => {
  const grid = slotGrid([record], 0, 30, (pokemon) => {
    const button = document.createElement('button');
    button.textContent = pokemon.nickname;
    return button;
  });
  expect(visibleSlots([record], 0, 30).filter(Boolean)).toHaveLength(1);
  expect(grid.querySelectorAll('.slot')).toHaveLength(30);
  expect(grid.querySelectorAll('.vacant')).toHaveLength(29);
  expect(grid.querySelectorAll('button')).toHaveLength(1);
  expect(grid.querySelectorAll('.slot-number')[7].textContent).toBe('8');
  expect(grid.querySelector('img')).toBeNull();
});

test('preview can apply only to the same save, record, box, slot and revision', () => {
  const context = { save_id: 3, vault_id: 7, box_index: 0, slot: 8 };
  const bound: BoundPreview = {
    context,
    result: {
      revision: 4,
      destination_slot_empty: true,
      report: {
        mode: 'copy',
        fully_compatible_fields: [],
        converted_fields: [],
        unsupported_fields: [],
        required_dependencies: [],
        blocking_problems: [],
        reversible: true,
      },
    },
  };
  expect(canApplyPreview(bound, context, 4)).toBe(true);
  expect(canApplyPreview(bound, { ...context, slot: 9 }, 4)).toBe(false);
  expect(canApplyPreview(bound, { ...context, save_id: 5 }, 4)).toBe(false);
  expect(canApplyPreview(bound, context, 5)).toBe(false);
  expect(
    canApplyPreview(
      { ...bound, result: { ...bound.result, destination_slot_empty: false } },
      context,
      4,
    ),
  ).toBe(false);
  expect(
    canApplyPreview(
      {
        ...bound,
        result: {
          ...bound.result,
          report: { ...bound.result.report, blocking_problems: ['blocked'] },
        },
      },
      context,
      4,
    ),
  ).toBe(false);
});

test('undo is unavailable after withdrawal undo even with an older active deposit', () => {
  const history = [
    { id: 3, save_id: 2, before_revision: 2, after_revision: 3, kind: 'undo', undone: false },
    { id: 2, save_id: 2, before_revision: 1, after_revision: 2, kind: 'withdraw', undone: true },
    {
      id: 1,
      save_id: 2,
      before_revision: 0,
      after_revision: 1,
      kind: 'copy-import',
      undone: false,
    },
  ];
  expect(canUndo(history, 2, 3)).toBe(false);
  expect(canUndo(history, 2, 1)).toBe(true);
  expect(canUndo(history, 4, 1)).toBe(false);
  expect(
    canUndo(
      [
        {
          id: 4,
          save_id: 2,
          before_revision: 3,
          after_revision: 4,
          kind: 'session',
          undone: false,
        },
      ],
      2,
      4,
    ),
  ).toBe(false);
});
