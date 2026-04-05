// --- Preferences ---

export interface Preferences {
  pagerunnerPath: string;
  daemonAutoStart: boolean;
}

// --- CLI Response Types ---

export interface Session {
  id: string;
  profile: string;
  display_name: string;
  stealth: boolean;
  status: "alive" | "crashed";
}

export interface Profile {
  name: string;
  display_name: string;
  kind: string;
  user_data_dir: string;
}

export interface Tab {
  target_id: string;
  url: string;
  title: string;
}

export interface KvEntry {
  key: string;
  value?: string;
}

export interface Checkpoint {
  checkpoint_id: string;
  name: string;
  saved_at: number;
  profile: string;
  tab_count: number;
  origins: string[];
}

export interface SiteKnowledge {
  origin: string;
  adapters: Record<string, {
    description: string;
    trusted: boolean;
    js_code: string;
    params_schema: unknown;
    last_used: number;
  }>;
  selectors: Array<{
    selector: string;
    successes: number;
    failures: number;
    reliability: number;
  }>;
  auth_tokens: Record<string, string>;
  endpoints: Array<{
    key: string;
    method: string;
    path_pattern: string;
    api_kind: string;
    crud_op: string | null;
    observations: number;
    has_schema: boolean;
  }>;
}

// --- CLI Response Envelopes ---

export interface OkResponse<T> {
  ok: true;
  data: T;
}

export interface SecretsResponse {
  secrets: string[];
}

export interface KvGetResponse {
  ok: true;
  key: string;
  value: string | null;
}

export interface OpenSessionResponse {
  ok: true;
  session_id: string;
  stealth: boolean;
}

export interface CloseSessionResponse {
  ok: true;
  session_id: string;
}

export interface ScreenshotResponse {
  base64: string;
}
