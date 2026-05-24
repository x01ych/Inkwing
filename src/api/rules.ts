import { invoke } from './tauri';

export interface Matcher {
  kind: string;
  values: string[];
}

export interface RuleView {
  id: string;
  editable: boolean;
  matchers: Matcher[];
  outbound: string | null;
  action: string | null;
  invert: boolean;
  readonly_reason: string;
  raw_pretty: string;
}

export interface RuleInput {
  matchers: Matcher[];
  outbound?: string | null;
  action?: string | null;
  invert?: boolean;
}

/** Where this rule came from in the merged overrides view. */
export type RuleSource = 'config' | 'local_per' | 'local_global';

/** What rules_list returns now: a RuleView wrapped with override metadata
 * so the UI can render the right badge / action set. */
export interface RuleViewWithBadge {
  id: string; // signature for source rules, UUID for local
  view: RuleView;
  source: RuleSource;
  modified: boolean;
  masked: boolean;
  /** Original signature, only present when modified=true (so the UI can
   * call revertRule on the right key). */
  original_signature: string | null;
}

/** Where a newly-added local rule lives. */
export type Scope = 'per_config' | 'global';

export const KNOWN_MATCHERS = [
  'domain',
  'domain_suffix',
  'domain_keyword',
  'domain_regex',
  'geosite',
  'geoip',
  'ip_cidr',
  'source_ip_cidr',
  'ip_is_private',
  'source_ip_is_private',
  'port',
  'port_range',
  'source_port',
  'source_port_range',
  'network',
  'protocol',
  'process_name',
  'process_path',
  'package_name',
  'user',
  'user_id',
  'inbound',
  'clash_mode',
  'rule_set',
] as const;

export type KnownMatcher = (typeof KNOWN_MATCHERS)[number];

export const MATCHER_GROUPS: { label: string; kinds: KnownMatcher[] }[] = [
  { label: 'Domain', kinds: ['domain', 'domain_suffix', 'domain_keyword', 'domain_regex', 'geosite'] },
  { label: 'IP', kinds: ['ip_cidr', 'source_ip_cidr', 'geoip', 'ip_is_private', 'source_ip_is_private'] },
  { label: 'Port / Net', kinds: ['port', 'port_range', 'source_port', 'source_port_range', 'network', 'protocol'] },
  { label: 'Process', kinds: ['process_name', 'process_path', 'package_name', 'user', 'user_id'] },
  { label: 'Inbound / Mode / Set', kinds: ['inbound', 'clash_mode', 'rule_set'] },
];

export const ACTIONS = ['route', 'block', 'reject', 'sniff', 'resolve', 'hijack-dns'] as const;

export const rulesApi = {
  list: () => invoke<RuleViewWithBadge[]>('rules_list'),
  add: (rule: RuleInput, scope: Scope) =>
    invoke<RuleViewWithBadge[]>('rules_add', { rule, scope }),
  update: (id: string, rule: RuleInput) =>
    invoke<RuleViewWithBadge[]>('rules_update', { id, rule }),
  /** Only valid for local rules (UUID id). Source-config rules return
   * an error — use mask instead. */
  delete: (id: string) => invoke<RuleViewWithBadge[]>('rules_delete', { id }),
  /** Hide a source-config rule from the merged runtime config. */
  mask: (signatureId: string) =>
    invoke<RuleViewWithBadge[]>('rules_mask', { signatureId }),
  unmask: (signatureId: string) =>
    invoke<RuleViewWithBadge[]>('rules_unmask', { signatureId }),
  /** Drop the modification override for a source-config rule, restoring
   * the original. */
  revert: (signatureId: string) =>
    invoke<RuleViewWithBadge[]>('rules_revert', { signatureId }),
  reorder: (ids: string[]) => invoke<RuleViewWithBadge[]>('rules_reorder', { ids }),
  commit: (restart: boolean) => invoke<void>('rules_commit', { restart }),
};

// ---- rule_set --------------------------------------------------------

export interface RuleSetView {
  id: string;
  editable: boolean;
  tag: string;
  kind: string; // "local" | "remote" | unknown
  format: string; // "binary" | "source"
  url: string | null;
  path: string | null;
  download_detour: string | null;
  update_interval: string | null;
  readonly_reason: string;
  raw_pretty: string;
  /** UNIX millis from sing-box's cache.db. Null if not yet downloaded. */
  last_updated_ms: number | null;
  /** HTTP ETag sing-box last saw. Null when no cache entry. */
  etag: string | null;
}

export interface RuleSetRefreshResult {
  tag: string;
  ok: boolean;
  new_last_updated_ms: number | null;
  error: string | null;
}

export interface RouteProbeReport {
  config_loaded: boolean;
  config_path: string | null;
  has_route: boolean;
  /** All keys directly under `route` in the user's source config. */
  route_keys: string[];
  has_route_rule_set: boolean;
  route_rule_set_is_array: boolean;
  route_rule_set_len: number;
  /** Keys that look like a typo'd version of `rule_set` (e.g. `ruleset`). */
  similar_route_keys: string[];
  /** Counts so we can tell users their rules reference rule_sets they
   * never defined. */
  rules_using_rule_set_matcher: number;
  rules_total: number;
}

export interface RuleSetInput {
  tag: string;
  kind: 'local' | 'remote';
  format: 'binary' | 'source';
  url?: string | null;
  path?: string | null;
  download_detour?: string | null;
  update_interval?: string | null;
}

export interface RuleSetViewWithBadge {
  id: string;
  view: RuleSetView;
  source: RuleSource;
  modified: boolean;
  masked: boolean;
  original_signature: string | null;
}

export const ruleSetsApi = {
  list: () => invoke<RuleSetViewWithBadge[]>('rule_sets_list'),
  add: (ruleSet: RuleSetInput, scope: Scope) =>
    invoke<RuleSetViewWithBadge[]>('rule_sets_add', { ruleSet, scope }),
  update: (id: string, ruleSet: RuleSetInput) =>
    invoke<RuleSetViewWithBadge[]>('rule_sets_update', { id, ruleSet }),
  delete: (id: string) => invoke<RuleSetViewWithBadge[]>('rule_sets_delete', { id }),
  mask: (signatureId: string) =>
    invoke<RuleSetViewWithBadge[]>('rule_sets_mask', { signatureId }),
  unmask: (signatureId: string) =>
    invoke<RuleSetViewWithBadge[]>('rule_sets_unmask', { signatureId }),
  revert: (signatureId: string) =>
    invoke<RuleSetViewWithBadge[]>('rule_sets_revert', { signatureId }),
  commit: (restart: boolean) =>
    invoke<void>('rule_sets_commit', { restart }),
  /** Force re-download of one remote rule_set. Restarts sing-box. */
  refresh: (tag: string) =>
    invoke<RuleSetRefreshResult>('rule_set_refresh', { tag }),
  /** Wipes all cached rule_sets and restarts sing-box; each remote
   * rule_set then re-downloads on startup. */
  refreshAll: () => invoke<RuleSetRefreshResult[]>('rule_set_refresh_all'),
  /** Diagnostic probe of the active config's `route` section so the
   * empty Rule Sets tab can explain WHY it's empty. */
  probe: () => invoke<RouteProbeReport>('rule_sets_probe'),
};
