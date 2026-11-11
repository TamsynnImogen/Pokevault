// @vitest-environment jsdom
import { beforeEach, expect, test, vi } from 'vitest';

const requestMock = vi.hoisted(() => vi.fn());
const chooseFileMock = vi.hoisted(() => vi.fn());
vi.mock('./transport', () => ({ request: requestMock, chooseFile: chooseFileMock }));

beforeEach(() => {
  vi.resetModules();
  document.body.replaceChildren();
  requestMock.mockReset();
  chooseFileMock.mockReset();
});

function clickButton(app: HTMLElement, label: string): void {
  const button = [...app.querySelectorAll('button')].find((item) => item.textContent === label);
  expect(button).toBeDefined();
  button!.click();
}

test('renders an untrusted ROM title as text while loading real IPC reads', async () => {
  const hostile = '<img src=x onerror=alert(1)>';
  requestMock.mockImplementation(async (action: string) => {
    if (action === 'overview')
      return {
        workspace_path: '/tmp/vault',
        vault_count: 0,
        saves: [],
        games: [
          {
            hash: 'abc',
            source_path: '/tmp/game.gba',
            title: hostile,
            platform: 'GBA',
            game_code: null,
            classification: 'unknown',
            import_supported: false,
            diagnostics: [],
          },
        ],
      };
    if (action === 'vault_list' || action === 'history') return [];
    throw new Error(`Unexpected action ${action}`);
  });
  const app = document.createElement('div');
  app.id = 'app';
  document.body.appendChild(app);
  await import('./main');
  await vi.waitFor(() => expect(app.textContent).toContain(hostile));
  expect(app.querySelector('img')).toBeNull();
  expect(requestMock.mock.calls.map((call) => call[0])).toEqual([
    'overview',
    'vault_list',
    'history',
  ]);
});

test('startup selects a linked save ahead of older unlinked entries', async () => {
  requestMock.mockImplementation(async (action: string) => {
    if (action === 'overview')
      return {
        workspace_path: '/tmp/vault',
        vault_count: 0,
        games: [],
        saves: [
          {
            id: 1,
            revision: 0,
            game_hash: 'old',
            definition_id: 'old',
            party_count: 0,
            pc_count: 0,
            warnings: [],
            linked_path: null,
            sync_status: 'unlinked',
          },
          {
            id: 7,
            revision: 0,
            game_hash: 'rom',
            definition_id: 'new',
            party_count: 0,
            pc_count: 0,
            warnings: [],
            linked_path: '/tmp/play.sav',
            sync_status: 'linked',
          },
        ],
      };
    if (action === 'save_view')
      return {
        id: 7,
        revision: 0,
        profile: {},
        warnings: [],
        pokemon: [],
        box_count: 1,
        slots_per_box: 30,
      };
    if (action === 'vault_list' || action === 'history') return [];
    throw new Error(`Unexpected action ${action}`);
  });
  const app = document.createElement('div');
  app.id = 'app';
  document.body.appendChild(app);
  await import('./main');
  await vi.waitFor(() => expect(requestMock).toHaveBeenCalledWith('save_view', { save_id: 7 }));
  expect(requestMock).not.toHaveBeenCalledWith('save_view', { save_id: 1 });
});

test('a save-view failure still leaves ROM library actions and other reads available', async () => {
  requestMock.mockImplementation(async (action: string) => {
    if (action === 'overview')
      return {
        workspace_path: '/tmp/vault',
        vault_count: 0,
        games: [],
        saves: [
          {
            id: 3,
            revision: 0,
            game_hash: 'hash',
            definition_id: 'emerald',
            party_count: null,
            pc_count: null,
            warnings: [],
          },
        ],
      };
    if (action === 'save_view') throw new Error('Add matching ROM');
    if (action === 'vault_list' || action === 'history') return [];
    throw new Error(`Unexpected action ${action}`);
  });
  const app = document.createElement('div');
  app.id = 'app';
  document.body.appendChild(app);
  await import('./main');
  await vi.waitFor(() => expect(app.textContent).toContain('Add matching ROM'));
  expect(app.textContent).toContain('Game library');
  expect(
    [...app.querySelectorAll('button')].some((button) => button.textContent === 'Add ROM'),
  ).toBe(true);
  expect(requestMock.mock.calls.map((call) => call[0])).toEqual([
    'overview',
    'save_view',
    'vault_list',
    'history',
  ]);
});

test('choosing a save selects the returned managed ID before refreshing', async () => {
  let selected = false;
  chooseFileMock.mockResolvedValue('/tmp/play.sav');
  requestMock.mockImplementation(async (action: string) => {
    if (action === 'overview')
      return {
        workspace_path: '/tmp/vault',
        vault_count: 0,
        games: [
          {
            hash: 'rom',
            source_path: '/tmp/game.gba',
            title: 'Game',
            platform: 'GBA',
            game_code: null,
            classification: 'known',
            import_supported: true,
            diagnostics: [],
          },
        ],
        saves: [
          {
            id: 1,
            revision: 0,
            game_hash: 'rom',
            definition_id: 'old',
            party_count: 0,
            pc_count: 0,
            warnings: [],
            linked_path: '/tmp/old.sav',
            sync_status: 'linked',
          },
          ...(selected
            ? [
                {
                  id: 2,
                  revision: 0,
                  game_hash: 'rom',
                  definition_id: 'new',
                  party_count: 0,
                  pc_count: 0,
                  warnings: [],
                  linked_path: '/tmp/play.sav',
                  sync_status: 'linked',
                },
              ]
            : []),
        ],
      };
    if (action === 'save_view')
      return {
        id: selected ? 2 : 1,
        revision: 0,
        profile: {},
        warnings: [],
        pokemon: [],
        box_count: 1,
        slots_per_box: 30,
      };
    if (action === 'choose_save') {
      selected = true;
      return { save_id: 2, revision: 0, message: 'Save linked.' };
    }
    if (action === 'vault_list' || action === 'history') return [];
    throw new Error(`Unexpected action ${action}`);
  });
  const app = document.createElement('div');
  app.id = 'app';
  document.body.appendChild(app);
  await import('./main');
  await vi.waitFor(() => expect(app.textContent).toContain('Choose save'));
  clickButton(app, 'Choose save');
  await vi.waitFor(() => expect(requestMock).toHaveBeenCalledWith('save_view', { save_id: 2 }));
  expect(requestMock).toHaveBeenCalledWith('choose_save', {
    rom_hash: 'rom',
    path: '/tmp/play.sav',
  });
});

test('remove game uses its hash and leaves an unlinked save transfer disabled', async () => {
  let removed = false;
  requestMock.mockImplementation(async (action: string) => {
    if (action === 'overview')
      return {
        workspace_path: '/tmp/vault',
        vault_count: 0,
        games: removed
          ? []
          : [
              {
                hash: 'rom',
                source_path: '/tmp/game.gba',
                title: 'Game',
                platform: 'GBA',
                game_code: null,
                classification: 'known',
                import_supported: true,
                diagnostics: [],
              },
            ],
        saves: [
          {
            id: 1,
            revision: 0,
            game_hash: 'rom',
            definition_id: 'old',
            party_count: 0,
            pc_count: 0,
            warnings: [],
            linked_path: null,
            sync_status: 'unlinked',
          },
        ],
      };
    if (action === 'save_view')
      return {
        id: 1,
        revision: 0,
        profile: {},
        warnings: [],
        pokemon: [],
        box_count: 1,
        slots_per_box: 30,
      };
    if (action === 'remove_game') {
      removed = true;
      return { message: 'Removed' };
    }
    if (action === 'vault_list' || action === 'history') return [];
    throw new Error(`Unexpected action ${action}`);
  });
  const app = document.createElement('div');
  app.id = 'app';
  document.body.appendChild(app);
  await import('./main');
  await vi.waitFor(() => expect(app.textContent).toContain('Remove game'));
  clickButton(app, 'Remove game');
  await vi.waitFor(() => expect(app.textContent).toContain('No games yet'));
  expect(requestMock).toHaveBeenCalledWith('remove_game', { rom_hash: 'rom' });
  clickButton(app, 'Managed save');
  const deposit = [...app.querySelectorAll('button')].find(
    (item) => item.textContent === 'Deposit selected to Vault',
  );
  expect(deposit?.disabled).toBe(true);
  expect(app.textContent).toContain('Choose a save in the game library');
});

test('failed save write refreshes pending status and keeps the error visible', async () => {
  let pending = false;
  requestMock.mockImplementation(async (action: string) => {
    if (action === 'overview')
      return {
        workspace_path: '/tmp/vault',
        vault_count: 0,
        games: [],
        saves: [
          {
            id: 1,
            revision: pending ? 2 : 1,
            game_hash: 'rom',
            definition_id: 'old',
            party_count: 0,
            pc_count: 0,
            warnings: [],
            linked_path: '/tmp/play.sav',
            sync_status: pending ? 'pending' : 'linked',
          },
        ],
      };
    if (action === 'save_view')
      return {
        id: 1,
        revision: pending ? 2 : 1,
        profile: {},
        warnings: [],
        pokemon: [],
        box_count: 1,
        slots_per_box: 30,
        external_sha256: pending ? 'external-hash' : null,
      };
    if (action === 'history')
      return [
        {
          id: 1,
          save_id: 1,
          before_revision: 0,
          after_revision: pending ? 2 : 1,
          kind: 'managed-deposit',
          undone: false,
        },
      ];
    if (action === 'undo') {
      pending = true;
      throw new Error('External save write failed');
    }
    if (action === 'recover_save') {
      pending = false;
      return { message: 'Recovered' };
    }
    if (action === 'vault_list') return [];
    throw new Error(`Unexpected action ${action}`);
  });
  const app = document.createElement('div');
  app.id = 'app';
  document.body.appendChild(app);
  await import('./main');
  await vi.waitFor(() => expect(app.textContent).toContain('Game library'));
  clickButton(app, 'Managed save');
  const undo = [...app.querySelectorAll('button')].find(
    (item) => item.textContent === 'Undo latest',
  );
  expect(undo?.disabled).toBe(false);
  undo!.click();
  await vi.waitFor(() => expect(app.textContent).toContain('External save write failed'));
  expect(app.textContent).toContain('needs attention (pending)');
  expect(
    [...app.querySelectorAll('button')].find((item) => item.textContent === 'Undo latest')
      ?.disabled,
  ).toBe(true);
  clickButton(app, 'Restore chosen save from Vault state');
  await vi.waitFor(() =>
    expect(requestMock).toHaveBeenCalledWith('recover_save', {
      save_id: 1,
      expected_revision: 2,
      expected_file_sha256: 'external-hash',
    }),
  );
});
