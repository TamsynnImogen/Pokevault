/** Public desktop IPC shapes from docs/DESKTOP_API.md. Keep source-local IDs unmodified. */
export type Location = { Party: { slot: number } } | { Box: { box_index: number; slot: number } };
export type Game = {
  hash: string;
  source_path: string;
  title: string;
  platform: string;
  game_code: string | null;
  classification: string;
  import_supported: boolean;
  diagnostics: string[];
};
export type SaveSummary = {
  id: number;
  revision: number;
  game_hash: string;
  definition_id: string;
  party_count: number | null;
  pc_count: number | null;
  warnings: string[];
  linked_path?: string | null;
  sync_status?: string;
};
export type Pokemon = {
  location: Location | null;
  vault_id: number | null;
  uuid: string | null;
  species_id: number;
  species_name: string;
  nickname: string;
  level: number | null;
  shiny: boolean;
  type_names: string[];
  source_game: string;
  raw_sha256: string;
  moves: { id: number; name: string; pp: number; pp_ups: number; definition: unknown }[];
  individual: unknown;
  species: unknown;
  evolutions: {
    from_name: string;
    to_name: string;
    method_namespace: string;
    requirements: unknown;
  }[];
  limitations: string[];
};
export type SaveView = {
  id: number;
  revision: number;
  profile: unknown;
  warnings: string[];
  pokemon: Pokemon[];
  box_count: number;
  slots_per_box: number;
  linked_path?: string | null;
  sync_status?: string;
  external_sha256?: string | null;
};
export type Overview = {
  workspace_path: string;
  games: Game[];
  saves: SaveSummary[];
  vault_count: number;
};
export type EmulatorStatus = {
  active: boolean;
  session_id: string | null;
  save_id: number | null;
  state: 'running' | 'paused' | 'finished' | 'save_pending' | 'stopped' | 'starting';
  core_name?: string;
  fps?: number;
  sample_rate?: number;
  frames?: number;
  error?: string | null;
};
export type Report = {
  mode: string;
  fully_compatible_fields: unknown;
  converted_fields: unknown;
  unsupported_fields: unknown;
  required_dependencies: unknown;
  blocking_problems: unknown;
  reversible: boolean;
};
export type Preview = { report: Report; revision: number; destination_slot_empty: boolean };
export type Operation = {
  id: number;
  save_id: number;
  before_revision: number;
  after_revision: number;
  kind: string;
  undone: boolean;
};

export function locationKey(location: Location): string {
  return 'Party' in location
    ? `party:${location.Party.slot}`
    : `box:${location.Box.box_index}:${location.Box.slot}`;
}

export function locationLabel(location: Location | null): string {
  if (!location) return 'Vault';
  return 'Party' in location
    ? `Party ${location.Party.slot + 1}`
    : `Box ${location.Box.box_index + 1} · ${location.Box.slot + 1}`;
}

export function entries(value: unknown): string[] {
  if (value == null) return [];
  if (Array.isArray(value))
    return value.map((item) => (typeof item === 'string' ? item : JSON.stringify(item)));
  if (typeof value === 'object')
    return Object.entries(value).map(
      ([key, item]) => `${key}: ${typeof item === 'string' ? item : JSON.stringify(item)}`,
    );
  return [String(value)];
}
