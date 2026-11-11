import './style.css';
import { EmulatorPage } from './emulator';
import { chooseFile, request } from './transport';
import {
  entries,
  locationKey,
  locationLabel,
  type Game,
  type Location,
  type Operation,
  type Overview,
  type Pokemon,
  type Preview,
  type SaveView,
} from './types';
import {
  canApplyPreview,
  canUndo,
  sameContext,
  slotGrid,
  visibleSlots,
  type BoundPreview,
  type PreviewContext,
} from './workflow';

type Page = 'library' | 'save' | 'vault' | 'activity' | 'tools' | 'emulator';
const root = document.querySelector<HTMLDivElement>('#app')!;
const emulator = new EmulatorPage(reload);
const state: {
  overview: Overview | null;
  save: SaveView | null;
  vault: Pokemon[];
  history: Operation[];
  page: Page;
  saveId: number | null;
  box: number;
  selected: Set<string>;
  detail: Pokemon | null;
  query: string;
  preview: BoundPreview | null;
  targetBox: number;
  targetSlot: number;
  busy: boolean;
  error: string;
  notice: string;
  corePath: string;
  diagnosticGameHash: string;
  frames: number;
  useSave: boolean;
} = {
  overview: null,
  save: null,
  vault: [],
  history: [],
  page: 'library',
  saveId: null,
  box: 0,
  selected: new Set(),
  detail: null,
  query: '',
  preview: null,
  targetBox: 0,
  targetSlot: 0,
  busy: false,
  error: '',
  notice: '',
  corePath: '',
  diagnosticGameHash: '',
  frames: 600,
  useSave: true,
};

/** DOM builders always use textContent; imported names and diagnostics are untrusted. */
function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = '',
  label?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (label !== undefined) node.textContent = label;
  return node;
}
function add(parent: Node, ...children: Node[]): void {
  children.forEach((child) => parent.appendChild(child));
}
function btn(
  label: string,
  click: () => unknown,
  className = '',
  disabled = false,
): HTMLButtonElement {
  const node = el('button', className, label);
  node.type = 'button';
  node.disabled = disabled || state.busy;
  node.addEventListener('click', () => {
    void click();
  });
  return node;
}
function card(title: string, description?: string): HTMLElement {
  const node = el('section', 'card');
  add(node, el('h2', '', title));
  if (description) add(node, el('p', 'muted', description));
  return node;
}
function list(parent: Node, values: string[], empty = 'None'): void {
  if (!values.length) {
    add(parent, el('p', 'muted', empty));
    return;
  }
  const ul = el('ul', 'plain-list');
  values.forEach((value) => add(ul, el('li', '', value)));
  add(parent, ul);
}
function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Mutations refresh every dependent read. A stale revision becomes a visible error. */
async function task<T>(work: () => Promise<T>, success?: (result: T) => string): Promise<T | null> {
  if (state.busy) return null;
  state.busy = true;
  state.error = '';
  state.notice = '';
  render();
  try {
    const result = await work();
    if (success) state.notice = success(result);
    return result;
  } catch (error) {
    state.error = errorText(error);
    return null;
  } finally {
    state.busy = false;
    render();
  }
}
async function refresh(): Promise<void> {
  const overview = await request<Overview>('overview');
  state.overview = overview;
  if (state.saveId != null && !overview.saves.some((save) => save.id === state.saveId))
    state.saveId = null;
  if (state.saveId == null && overview.saves.length)
    state.saveId = (
      overview.saves.find((save) => ['linked', 'emulating'].includes(save.sync_status ?? '')) ??
      overview.saves[0]
    ).id;
  if (state.saveId == null) state.save = null;
  else {
    try {
      state.save = await request<SaveView>('save_view', { save_id: state.saveId });
    } catch (error) {
      state.save = null;
      state.error = `Managed save ${state.saveId} needs attention: ${errorText(error)}`;
    }
  }
  state.vault = await request<Pokemon[]>('vault_list', { query: state.query });
  state.history = await request<Operation[]>('history', { save_id: state.saveId });
  state.selected.clear();
  state.preview = null;
  state.detail = null;
}
async function reload(): Promise<void> {
  await task(refresh);
}
async function mutate(
  action: string,
  fields: Record<string, unknown>,
  message: string,
): Promise<void> {
  await task(
    async () => {
      try {
        await request(action, fields);
      } catch (error) {
        // A failed external write can leave a pending managed revision. Read
        // it again before permitting any further transfer, but keep the cause.
        try {
          await refresh();
        } catch {
          // The original mutation error is the actionable result.
        }
        throw error;
      }
      await refresh();
    },
    () => message,
  );
}
async function addRom(): Promise<void> {
  const path = await task(() => chooseFile(['gba', 'gbc', 'gb']));
  if (!path) return;
  await task(
    async () => {
      await request<Game>('add_rom', { path });
      await refresh();
    },
    () => 'ROM added to the library.',
  );
}
async function chooseSave(game: Game): Promise<void> {
  const path = await task(() => chooseFile(['sav', 'srm']));
  if (!path) return;
  await task(
    async () => {
      const result = await request<{ save_id: number; revision: number; message: string }>(
        'choose_save',
        { rom_hash: game.hash, path },
      );
      // A path may already have a managed ID. Select the returned ID before
      // refresh so the previous selection does not silently remain active.
      state.saveId = result.save_id;
      await refresh();
      return result;
    },
    (result) =>
      `${result.message} Close the emulator before editing this save; use title-screen Continue afterwards, not a savestate.`,
  );
}
async function removeGame(game: Game): Promise<void> {
  await mutate(
    'remove_game',
    { rom_hash: game.hash },
    'Game removed from the library. Saves, Vault Pokémon, and files remain available.',
  );
}
function linkedSaveReady(): boolean {
  const summary = state.overview?.saves.find((item) => item.id === state.saveId);
  return !!summary?.linked_path && summary.sync_status === 'linked';
}
function linkedSaveNote(): string {
  const summary = state.overview?.saves.find((item) => item.id === state.saveId);
  if (!summary?.linked_path) return 'Choose a save in the game library before changing Pokémon.';
  if (summary.sync_status === 'emulating')
    return 'Stop & save in Play game before changing Pokémon.';
  return `The selected save needs attention (${summary.sync_status ?? 'unknown'}). Close the emulator, then choose this save again before changing Pokémon.`;
}
async function deposit(): Promise<void> {
  if (!state.save) return;
  const locations = state.save.pokemon
    .filter((p) => p.location && state.selected.has(locationKey(p.location)))
    .map((p) => p.location as Location);
  if (!locations.length) return;
  await mutate(
    'deposit',
    {
      save_id: state.save.id,
      expected_revision: state.save.revision,
      locations,
      move_from_managed: true,
    },
    'Deposited into the Vault and removed from the selected save file. Use title-screen Continue in the emulator, not a savestate.',
  );
}
function currentPreviewContext(): PreviewContext | null {
  return state.save && state.detail?.vault_id != null
    ? {
        save_id: state.save.id,
        vault_id: state.detail.vault_id,
        box_index: state.targetBox,
        slot: state.targetSlot,
      }
    : null;
}
async function previewWithdraw(): Promise<void> {
  const context = currentPreviewContext();
  if (!context) return;
  state.preview = null;
  const result = await task(() => request<Preview>('preview_withdraw', context));
  state.preview =
    result && currentPreviewContext() && sameContext(context, currentPreviewContext()!)
      ? { context, result }
      : null;
  render();
}
async function applyWithdraw(): Promise<void> {
  const bound = state.preview;
  if (!canApplyPreview(bound, currentPreviewContext(), state.save?.revision ?? null)) return;
  await mutate(
    'withdraw',
    { ...bound!.context, expected_revision: bound!.result.revision },
    'Pokémon removed from the Vault and saved to the selected file. Use title-screen Continue in the emulator, not a savestate.',
  );
}

function render(): void {
  if (state.page !== 'emulator') emulator.hide();
  root.replaceChildren();
  const shell = el('div', 'shell');
  const sidebar = el('aside', 'sidebar');
  add(
    sidebar,
    el('div', 'brand', '◈  Pokémon Vault'),
    el('p', 'brand-sub', 'Your local collection'),
  );
  const nav = el('nav');
  (['library', 'save', 'vault', 'activity', 'tools', 'emulator'] as Page[]).forEach((page, index) =>
    add(
      nav,
      btn(
        ['Game library', 'Managed save', 'Vault', 'History', 'Tools', 'Play game'][index],
        () => {
          state.page = page;
          render();
        },
        `nav-button ${state.page === page ? 'active' : ''}`,
      ),
    ),
  );
  add(
    sidebar,
    nav,
    el(
      'div',
      'sidebar-foot',
      state.overview
        ? `${state.overview.vault_count} in vault · ${state.overview.saves.length} managed saves`
        : 'Local desktop workspace',
    ),
  );
  const main = el('main', 'main');
  const top = el('header', 'topbar');
  const heading = el('div');
  add(
    heading,
    el('p', 'eyebrow', 'LOCAL COLLECTION'),
    el(
      'h1',
      '',
      {
        library: 'Game library',
        save: 'Managed save',
        vault: 'The vault',
        activity: 'Operation history',
        tools: 'Workspace tools',
        emulator: 'Play game',
      }[state.page],
    ),
  );
  add(top, heading, btn('Refresh', reload, 'quiet'));
  add(main, top);
  if (state.busy) add(main, el('p', 'banner progress', 'Working with your local vault…'));
  if (state.error) add(main, el('p', 'banner error', state.error));
  if (state.notice) add(main, el('p', 'banner success', state.notice));
  if (!state.overview) {
    const empty = card(
      'Connect to your local collection',
      'Open the desktop application to load its private workspace.',
    );
    add(empty, btn('Try again', reload, 'primary'));
    add(main, empty);
  } else {
    if (state.page === 'library') library(main);
    if (state.page === 'save') savePage(main);
    if (state.page === 'vault') vaultPage(main);
    if (state.page === 'activity') activity(main);
    if (state.page === 'tools') toolsPage(main);
    if (state.page === 'emulator') {
      if (state.overview.saves.length) {
        const controls = el('div', 'toolbar');
        savePicker(controls, true);
        add(main, controls);
      }
      emulator.mount(main, state.overview, state.saveId);
    }
  }
  add(shell, sidebar, main);
  root.appendChild(shell);
}

function library(main: HTMLElement): void {
  const intro = el('div', 'intro-row');
  add(
    intro,
    el('p', 'muted', 'Add a ROM to identify its game, then choose the save file you play with.'),
    btn('Add ROM', addRom, 'primary'),
  );
  add(main, intro);
  const games = state.overview!.games;
  if (!games.length) {
    add(
      main,
      card('No games yet', 'Choose a local ROM to begin. Your original files stay where they are.'),
    );
    return;
  }
  const grid = el('div', 'cards');
  games.forEach((game) => {
    const node = card(game.title || 'Untitled game');
    add(
      node,
      el(
        'p',
        'meta',
        `${game.platform} · ${game.classification}${game.game_code ? ` · ${game.game_code}` : ''}`,
      ),
      el('p', 'path', game.source_path),
      el('p', 'digest', `ROM SHA-256  ${game.hash}`),
    );
    if (game.diagnostics.length) list(node, game.diagnostics);
    add(
      node,
      btn(
        game.import_supported ? 'Choose save' : 'Choose save unavailable',
        () => chooseSave(game),
        'secondary',
        !game.import_supported,
      ),
      btn('Remove game', () => removeGame(game), 'quiet'),
      el(
        'p',
        'muted small',
        'Removes this library entry only. Saves, Vault Pokémon, and files stay in place.',
      ),
    );
    add(grid, node);
  });
  add(main, grid);
}

function savePicker(parent: Node, lockWhileEmulating = false): void {
  const wrap = el('label', 'field', 'Managed save');
  const select = el('select');
  state.overview!.saves.forEach((save) => {
    const filename = save.linked_path?.split(/[\\/]/).pop();
    const option = el('option', '', `${filename ?? 'Unlinked save'} · Save ${save.id}`);
    option.value = String(save.id);
    option.selected = save.id === state.saveId;
    add(select, option);
  });
  select.disabled =
    state.busy ||
    (lockWhileEmulating && state.overview!.saves.some((save) => save.sync_status === 'emulating'));
  select.addEventListener('change', () => {
    state.saveId = Number(select.value);
    void reload();
  });
  add(wrap, select);
  add(parent, wrap);
}
function pokemonButton(pokemon: Pokemon, selected: boolean, click: () => void): HTMLButtonElement {
  const button = btn('', click, `pokemon-row ${selected ? 'selected' : ''}`);
  add(
    button,
    el('span', 'pokemon-icon', pokemon.shiny ? '✦' : '●'),
    el('span', 'pokemon-name', pokemon.nickname || pokemon.species_name),
    el(
      'span',
      'pokemon-sub',
      `${pokemon.species_name} · ${locationLabel(pokemon.location)}${pokemon.level == null ? '' : ` · Lv ${pokemon.level}`}`,
    ),
  );
  return button;
}
function savePage(main: HTMLElement): void {
  if (!state.overview!.saves.length) {
    add(main, card('No managed save', 'Choose a matching save in the game library first.'));
    return;
  }
  const controls = el('div', 'toolbar');
  savePicker(controls);
  add(
    controls,
    btn(
      'Undo latest',
      () =>
        state.save &&
        mutate(
          'undo',
          { save_id: state.save.id, expected_revision: state.save.revision },
          'Latest operation undone and saved back to the selected file. Use title-screen Continue, not a savestate.',
        ),
      'quiet',
      !linkedSaveReady() || !canUndo(state.history, state.saveId, state.save?.revision ?? null),
    ),
  );
  add(main, controls);
  const save = state.save;
  const summary = state.overview!.saves.find((item) => item.id === state.saveId);
  if (!save) {
    const counts = el('div', 'stat-band');
    add(
      counts,
      el('span', 'stat', `${summary?.party_count ?? '—'} party`),
      el('span', 'stat', `${summary?.pc_count ?? '—'} PC`),
    );
    add(
      main,
      counts,
      card(
        'Managed save unavailable',
        'Add its matching ROM in the game library, then refresh to inspect this save.',
      ),
    );
    return;
  }
  const band = el('div', 'stat-band');
  [
    `${summary?.party_count ?? '—'} party`,
    `${summary?.pc_count ?? '—'} PC`,
    `Revision ${save.revision}`,
    `${save.box_count} boxes`,
    summary?.definition_id ?? 'Unknown source',
  ].forEach((value) => add(band, el('span', 'stat', value)));
  add(main, band);
  if (linkedSaveReady())
    add(
      main,
      el(
        'p',
        'muted small',
        `Selected file: ${summary?.linked_path}. Close the emulator before changing Pokémon; afterwards use title-screen Continue, not a savestate.`,
      ),
    );
  else add(main, el('p', 'banner error', linkedSaveNote()));
  if (summary?.sync_status === 'pending') {
    const recovery = card(
      'Pending save write',
      `Restore the chosen save file ${summary.linked_path ?? ''} from the committed Vault state. This replaces that file and backs up its current contents. Close the emulator first.`,
    );
    add(
      recovery,
      btn(
        'Restore chosen save from Vault state',
        () =>
          mutate(
            'recover_save',
            {
              save_id: save.id,
              expected_revision: save.revision,
              expected_file_sha256: save.external_sha256,
            },
            'Chosen save restored from the Vault state with a backup of its previous contents. Use title-screen Continue, not a savestate.',
          ),
        'secondary',
        !save.external_sha256,
      ),
    );
    add(main, recovery);
  }
  if (save.warnings.length) {
    const warnings = card('Save notes');
    list(warnings, save.warnings);
    add(main, warnings);
  }
  const layout = el('div', 'two-column');
  const browser = card('Choose Pokémon');
  const tabs = el('div', 'box-tabs');
  add(
    tabs,
    btn(
      'Party',
      () => {
        state.box = -1;
        state.selected.clear();
        render();
      },
      state.box === -1 ? 'active' : '',
    ),
  );
  for (let i = 0; i < save.box_count; i++)
    add(
      tabs,
      btn(
        String(i + 1),
        () => {
          state.box = i;
          state.selected.clear();
          render();
        },
        state.box === i ? 'active' : '',
      ),
    );
  add(browser, tabs);
  const slots = visibleSlots(save.pokemon, state.box, state.box === -1 ? 6 : save.slots_per_box);
  const relevant = slots.filter((p): p is Pokemon => p !== null);
  const rows = slotGrid(save.pokemon, state.box, slots.length, (pokemon) =>
    pokemonButton(
      pokemon,
      !!pokemon.location && state.selected.has(locationKey(pokemon.location)),
      () => {
        if (!pokemon.location) return;
        const key = locationKey(pokemon.location);
        state.selected.has(key) ? state.selected.delete(key) : state.selected.add(key);
        state.detail = pokemon;
        render();
      },
    ),
  );
  add(browser, rows);
  const actions = el('div', 'actions');
  add(
    actions,
    btn(
      state.box === -1 ? 'Select occupied party' : 'Select occupied box',
      () => {
        relevant.forEach((p) => p.location && state.selected.add(locationKey(p.location)));
        render();
      },
      'quiet',
      !relevant.length,
    ),
    btn(
      'Clear',
      () => {
        state.selected.clear();
        render();
      },
      'quiet',
      !state.selected.size,
    ),
  );
  add(
    browser,
    actions,
    el(
      'p',
      'muted small',
      `${state.selected.size} selected. Deposit moves Pokémon out of the selected save file into the Vault.`,
    ),
  );
  const moveBlocked = [...state.selected].some((key) => key.startsWith('party:'));
  add(
    browser,
    btn(
      'Deposit selected to Vault',
      deposit,
      'primary',
      !linkedSaveReady() || !state.selected.size || moveBlocked,
    ),
    el('p', 'muted small', 'A backup is made before the selected save file changes.'),
  );
  if (state.box === -1 || moveBlocked)
    add(
      browser,
      el(
        'p',
        'muted small',
        'Party deposits are not supported yet. Put the Pokémon in a PC box in-game, save, then choose that save again before depositing.',
      ),
    );
  const detail = card('Selected Pokémon', 'Choose an occupied slot to inspect its source data.');
  if (state.detail && state.detail.location) pokemonDetail(detail, state.detail);
  add(layout, browser, detail);
  add(main, layout);
  const profile = card(
    'Source profile',
    'This profile describes the managed save and its source rules.',
  );
  dataBlock(profile, save.profile);
  add(main, profile);
}

function pokemonDetail(parent: Node, pokemon: Pokemon): void {
  add(
    parent,
    el('h3', '', `${pokemon.nickname || pokemon.species_name}${pokemon.shiny ? ' ✦' : ''}`),
    el(
      'p',
      'meta',
      `${pokemon.species_name} #${pokemon.species_id} · ${pokemon.type_names.join(' / ') || 'Type unknown'}`,
    ),
    el(
      'p',
      'muted',
      `${locationLabel(pokemon.location)} · ${pokemon.source_game} · ${pokemon.level == null ? 'Level unknown' : `Level ${pokemon.level}`}`,
    ),
  );
  if (pokemon.uuid) add(parent, el('p', 'digest', `Identity  ${pokemon.uuid}`));
  if (pokemon.vault_id != null) add(parent, el('p', 'digest', `Vault ID  ${pokemon.vault_id}`));
  add(parent, el('p', 'digest', `Record SHA-256  ${pokemon.raw_sha256}`));
  add(parent, el('h4', '', 'Moves'));
  list(
    parent,
    pokemon.moves.map(
      (move) => `${move.name} (#${move.id}) · PP ${move.pp} · PP Ups ${move.pp_ups}`,
    ),
    'No moves recorded.',
  );
  add(parent, el('h4', '', 'Individual data'));
  dataBlock(parent, pokemon.individual);
  add(parent, el('h4', '', 'Source species rules'));
  dataBlock(parent, pokemon.species);
  add(parent, el('h4', '', 'Evolutions'));
  list(
    parent,
    pokemon.evolutions.map(
      (evo) =>
        `${evo.from_name} → ${evo.to_name} · ${evo.method_namespace} · ${entries(evo.requirements).join(', ')}`,
    ),
    'None recorded.',
  );
  add(parent, el('h4', '', 'Limitations'));
  list(parent, pokemon.limitations, 'None reported.');
}
function dataBlock(parent: Node, value: unknown): void {
  const pre = el('pre', 'data-block', JSON.stringify(value, null, 2) ?? 'Unavailable');
  add(parent, pre);
}

function vaultPage(main: HTMLElement): void {
  const search = el('input');
  search.type = 'search';
  search.placeholder = 'Search species, nickname, UUID…';
  search.value = state.query;
  search.disabled = state.busy;
  const searchForm = el('form', 'search-form');
  const searchWrap = el('label', 'search-field');
  add(searchWrap, el('span', '', 'Search vault'), search);
  const submit = btn('Search', () => {}, 'secondary');
  submit.type = 'submit';
  add(searchForm, searchWrap, submit);
  searchForm.addEventListener('submit', (event) => {
    event.preventDefault();
    state.query = search.value;
    state.detail = null;
    state.preview = null;
    void task(async () => {
      state.vault = await request<Pokemon[]>('vault_list', { query: state.query });
    });
  });
  add(main, searchForm);
  const layout = el('div', 'two-column');
  const results = card(`${state.vault.length} Pokémon`);
  if (!state.vault.length) add(results, el('p', 'empty', 'No Pokémon match this search.'));
  state.vault.forEach((p) =>
    add(
      results,
      pokemonButton(p, state.detail?.vault_id === p.vault_id, () => {
        state.detail = p;
        state.preview = null;
        render();
      }),
    ),
  );
  const detail = card('Vault detail', 'Select a Pokémon to inspect it and preview withdrawal.');
  if (state.detail?.vault_id != null) {
    pokemonDetail(detail, state.detail);
    withdrawal(detail);
  }
  add(layout, results, detail);
  add(main, layout);
}
function withdrawal(parent: Node): void {
  add(parent, el('h3', '', 'Withdraw to selected save'));
  if (!state.overview!.saves.length) {
    add(parent, el('p', 'muted', 'Choose a save in the game library before withdrawing.'));
    return;
  }
  savePicker(parent);
  const save = state.save;
  if (!save) return;
  if (!linkedSaveReady()) add(parent, el('p', 'banner error', linkedSaveNote()));
  else
    add(
      parent,
      el(
        'p',
        'muted small',
        `Selected file: ${state.overview!.saves.find((item) => item.id === state.saveId)?.linked_path}. Close the emulator before applying; afterwards use title-screen Continue, not a savestate.`,
      ),
    );
  const fields = el('div', 'target-fields');
  const boxLabel = el('label', 'field', 'Box');
  const box = el('select');
  for (let i = 0; i < save.box_count; i++) {
    const option = el('option', '', `Box ${i + 1}`);
    option.value = String(i);
    option.selected = i === state.targetBox;
    add(box, option);
  }
  box.disabled = state.busy;
  box.addEventListener('change', () => {
    state.targetBox = Number(box.value);
    state.preview = null;
    render();
  });
  add(boxLabel, box);
  const slotLabel = el('label', 'field', 'Slot');
  const slot = el('select');
  for (let i = 0; i < save.slots_per_box; i++) {
    const option = el('option', '', String(i + 1));
    option.value = String(i);
    option.selected = i === state.targetSlot;
    add(slot, option);
  }
  slot.disabled = state.busy;
  slot.addEventListener('change', () => {
    state.targetSlot = Number(slot.value);
    state.preview = null;
    render();
  });
  add(slotLabel, slot);
  add(fields, boxLabel, slotLabel);
  add(parent, fields);
  add(parent, btn('Preview compatibility', previewWithdraw, 'secondary'));
  if (state.preview) {
    const report = state.preview.result.report;
    const preview = el('div', 'preview');
    add(
      preview,
      el('h4', '', 'Compatibility report'),
      el(
        'p',
        state.preview.result.destination_slot_empty ? 'good' : 'bad',
        state.preview.result.destination_slot_empty
          ? 'Destination slot is empty'
          : 'Destination slot is occupied',
      ),
      el(
        'p',
        'meta',
        `Mode: ${report.mode} · ${report.reversible ? 'Reversible' : 'Irreversible'}`,
      ),
    );
    for (const [heading, value] of [
      ['Blocking problems', report.blocking_problems],
      ['Converted fields', report.converted_fields],
      ['Unsupported fields', report.unsupported_fields],
      ['Required dependencies', report.required_dependencies],
      ['Fully compatible fields', report.fully_compatible_fields],
    ] as const) {
      add(preview, el('h5', '', heading));
      list(preview, entries(value));
    }
    add(
      preview,
      btn(
        'Apply withdrawal',
        applyWithdraw,
        'primary',
        !linkedSaveReady() ||
          !canApplyPreview(state.preview, currentPreviewContext(), state.save?.revision ?? null),
      ),
    );
    add(parent, preview);
  }
}

function activity(main: HTMLElement): void {
  const controls = el('div', 'toolbar');
  if (state.overview!.saves.length) savePicker(controls);
  add(main, controls);
  const node = card(
    'Recent operations',
    'Newest first. An undone operation remains in the record.',
  );
  if (!state.history.length) add(node, el('p', 'empty', 'No operations recorded for this save.'));
  state.history.forEach((op) => {
    const row = el('div', 'history-row');
    add(
      row,
      el('strong', '', op.kind),
      el(
        'span',
        'muted',
        `Save ${op.save_id} · rev ${op.before_revision} → ${op.after_revision}${op.undone ? ' · undone' : ''}`,
      ),
    );
    add(node, row);
  });
  add(main, node);
}

function toolsPage(main: HTMLElement): void {
  const grid = el('div', 'cards');
  const output = card('Save and workspace');
  if (state.overview!.saves.length) savePicker(output);
  add(
    output,
    btn(
      'Export managed save',
      () => {
        const saveId = state.save?.id;
        if (saveId != null)
          void task(
            () =>
              request<{ path: string; save_sha256: string; message: string }>('export_save', {
                save_id: saveId,
              }),
            (result) => `Exported to ${result.path} · SHA-256 ${result.save_sha256}`,
          );
      },
      'secondary',
      !state.save,
    ),
    btn(
      'Back up vault database',
      () =>
        task(
          () => request<{ path: string }>('backup'),
          (result) => `Backup created at ${result.path}`,
        ),
      'secondary',
    ),
    btn(
      'Check integrity',
      () =>
        task(
          () => request<{ message: string }>('integrity_check'),
          (result) => result.message,
        ),
      'secondary',
    ),
  );
  add(
    output,
    el(
      'p',
      'muted small',
      'Export creates an optional separate copy. Linked transfers write to the selected save file after making a backup.',
    ),
  );
  const diagnostics = card(
    'Headless diagnostics',
    'Run real mGBA diagnostics without opening playable emulator controls.',
  );
  const gameSelect = el('select');
  if (!state.overview!.games.some((game) => game.hash === state.diagnosticGameHash))
    state.diagnosticGameHash = state.overview!.games[0]?.hash ?? '';
  state.overview!.games.forEach((game) => {
    const option = el('option', '', game.title || game.hash);
    option.value = game.hash;
    option.selected = game.hash === state.diagnosticGameHash;
    add(gameSelect, option);
  });
  gameSelect.disabled = state.busy;
  gameSelect.addEventListener('change', () => {
    state.diagnosticGameHash = gameSelect.value;
  });
  const core = el('label', 'field', 'mGBA core');
  const coreValue = el('input');
  coreValue.readOnly = true;
  coreValue.disabled = state.busy;
  coreValue.value = state.corePath;
  coreValue.placeholder = 'Choose core file';
  add(
    core,
    coreValue,
    btn(
      'Choose core',
      async () => {
        const path = await task(() => chooseFile(['so', 'dll', 'dylib']));
        if (path) {
          state.corePath = path;
          render();
        }
      },
      'quiet',
    ),
  );
  const framesLabel = el('label', 'field', 'Frames');
  const frames = el('input');
  frames.type = 'number';
  frames.min = '1';
  frames.max = '3600';
  frames.value = String(state.frames);
  frames.disabled = state.busy;
  add(framesLabel, frames);
  const useSave = el('label', 'check');
  const checkbox = el('input');
  checkbox.type = 'checkbox';
  checkbox.checked = state.useSave;
  checkbox.disabled = state.busy;
  checkbox.addEventListener('change', () => {
    state.useSave = checkbox.checked;
  });
  add(useSave, checkbox, el('span', '', 'Use selected managed save'));
  add(diagnostics, el('label', 'field', 'Game'), gameSelect, core, framesLabel, useSave);
  const run = btn(
    'Run headless diagnostics',
    () =>
      task(
        async () => {
          const result = await request<unknown>('run_headless', {
            rom_hash: state.diagnosticGameHash,
            save_id: state.useSave ? state.saveId : null,
            core_path: state.corePath,
            frames: state.frames,
          });
          // Managed diagnostics can commit a save revision. Refresh the same
          // dependent views as a transfer before accepting another selection.
          await refresh();
          return result;
        },
        (result) => `Diagnostics completed: ${JSON.stringify(result)}`,
      ),
    'secondary',
    !state.corePath ||
      !state.overview!.games.length ||
      !Number.isInteger(state.frames) ||
      state.frames < 1 ||
      state.frames > 3600,
  );
  frames.addEventListener('input', () => {
    state.frames = Number(frames.value);
    run.disabled =
      state.busy ||
      !state.corePath ||
      !Number.isInteger(state.frames) ||
      state.frames < 1 ||
      state.frames > 3600;
  });
  add(diagnostics, run);
  add(grid, output, diagnostics);
  add(main, grid);
}

render();
void reload();
