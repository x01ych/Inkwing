import { invoke } from './tauri';
import type { RuleSource, Scope } from './rules';

export const KNOWN_DNS_SERVER_TYPES = [
  'udp',
  'tcp',
  'tls',
  'https',
  'quic',
  'h3',
  'local',
  'hosts',
  'dhcp',
  'fakeip',
] as const;

export type DnsServerType = (typeof KNOWN_DNS_SERVER_TYPES)[number];

export interface DnsServerView {
  id: string;
  editable: boolean;
  tag: string;
  /** sing-box's `type` field. Renamed to `kind` to avoid the JS reserved
   * connotations + it matches our route module's term. */
  kind: string;
  server: string | null;
  server_port: number | null;
  path: string | null;
  detour: string | null;
  domain_resolver: string | null;
  domain_strategy: string | null;
  /** Legacy 1.11 form — `address: "tls://..."`. Read-only when set. */
  address: string | null;
  readonly_reason: string;
  raw_pretty: string;
}

export interface DnsServerInput {
  tag: string;
  kind: DnsServerType;
  server?: string | null;
  server_port?: number | null;
  path?: string | null;
  detour?: string | null;
  domain_resolver?: string | null;
  domain_strategy?: string | null;
  /** Pass-through for fields not yet surfaced in the form. */
  extra?: Record<string, unknown>;
}

export interface DnsServerViewWithBadge {
  id: string;
  view: DnsServerView;
  source: RuleSource;
  modified: boolean;
  masked: boolean;
  original_signature: string | null;
}

export const KNOWN_DNS_MATCHERS = [
  'domain',
  'domain_suffix',
  'domain_keyword',
  'domain_regex',
  'geosite',
  'rule_set',
  'source_ip_cidr',
  'source_ip_is_private',
  'port',
  'port_range',
  'source_port',
  'source_port_range',
  'process_name',
  'process_path',
  'package_name',
  'user',
  'user_id',
  'network',
  'protocol',
  'inbound',
  'clash_mode',
  'query_type',
] as const;

export type KnownDnsMatcher = (typeof KNOWN_DNS_MATCHERS)[number];

export const DNS_MATCHER_GROUPS: { label: string; kinds: KnownDnsMatcher[] }[] = [
  {
    label: 'Domain',
    kinds: ['domain', 'domain_suffix', 'domain_keyword', 'domain_regex', 'geosite', 'rule_set'],
  },
  {
    label: 'IP / Port',
    kinds: ['source_ip_cidr', 'source_ip_is_private', 'port', 'port_range', 'source_port', 'source_port_range'],
  },
  { label: 'Process', kinds: ['process_name', 'process_path', 'package_name', 'user', 'user_id'] },
  { label: 'DNS / Net', kinds: ['query_type', 'network', 'protocol', 'inbound', 'clash_mode'] },
];

export const DNS_ACTIONS = ['route', 'reject', 'predefined', 'route-options'] as const;

export interface DnsMatcher {
  kind: string;
  values: string[];
}

export interface DnsRuleView {
  id: string;
  editable: boolean;
  matchers: DnsMatcher[];
  server: string | null;
  action: string | null;
  invert: boolean;
  readonly_reason: string;
  raw_pretty: string;
}

export interface DnsRuleInput {
  matchers: DnsMatcher[];
  server?: string | null;
  action?: string | null;
  invert?: boolean;
}

export interface DnsRuleViewWithBadge {
  id: string;
  view: DnsRuleView;
  source: RuleSource;
  modified: boolean;
  masked: boolean;
  original_signature: string | null;
}

export const dnsServersApi = {
  list: () => invoke<DnsServerViewWithBadge[]>('dns_servers_list'),
  add: (server: DnsServerInput, scope: Scope) =>
    invoke<DnsServerViewWithBadge[]>('dns_servers_add', { server, scope }),
  update: (id: string, server: DnsServerInput) =>
    invoke<DnsServerViewWithBadge[]>('dns_servers_update', { id, server }),
  delete: (id: string) => invoke<DnsServerViewWithBadge[]>('dns_servers_delete', { id }),
  mask: (signatureId: string) =>
    invoke<DnsServerViewWithBadge[]>('dns_servers_mask', { signatureId }),
  unmask: (signatureId: string) =>
    invoke<DnsServerViewWithBadge[]>('dns_servers_unmask', { signatureId }),
  revert: (signatureId: string) =>
    invoke<DnsServerViewWithBadge[]>('dns_servers_revert', { signatureId }),
};

export const dnsRulesApi = {
  list: () => invoke<DnsRuleViewWithBadge[]>('dns_rules_list'),
  add: (rule: DnsRuleInput, scope: Scope) =>
    invoke<DnsRuleViewWithBadge[]>('dns_rules_add', { rule, scope }),
  update: (id: string, rule: DnsRuleInput) =>
    invoke<DnsRuleViewWithBadge[]>('dns_rules_update', { id, rule }),
  delete: (id: string) => invoke<DnsRuleViewWithBadge[]>('dns_rules_delete', { id }),
  mask: (signatureId: string) =>
    invoke<DnsRuleViewWithBadge[]>('dns_rules_mask', { signatureId }),
  unmask: (signatureId: string) =>
    invoke<DnsRuleViewWithBadge[]>('dns_rules_unmask', { signatureId }),
  revert: (signatureId: string) =>
    invoke<DnsRuleViewWithBadge[]>('dns_rules_revert', { signatureId }),
};

export const dnsCommit = (restart: boolean) => invoke<void>('dns_commit', { restart });
