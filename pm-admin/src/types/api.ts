export interface ConfigSnapshotMeta {
  config_snapshot_id: string;
  content_hash: string;
  version_label: string | null;
  is_active: boolean;
  file_count: number;
  name: string | null;
  created_at: string;
  activated_at: string | null;
}

export interface UploadSuccessResponse {
  valid: true;
  config_snapshot_id: string;
  name: string;
  content_hash: string;
  file_count: number;
  table_names: string[];
}

export interface UploadErrorResponse {
  valid: false;
  errors: string[];
  config_snapshot_id: string;
}

export type UploadResponse = UploadSuccessResponse | UploadErrorResponse;

export interface ActivateResponse {
  config_snapshot_id: string;
  active: true;
  content_hash: string;
  activated_at: string | null;
}

export interface AgentCapabilities {
  can_collect: boolean;
  can_parse: boolean;
  can_load: boolean;
  supported_protocols: string[];
}

export interface AgentRegisterRequest {
  agent_id: string | null;
  agent_name: string;
  host: string;
  port: number;
  version: string;
  capabilities: AgentCapabilities;
}

export interface AgentRegisterResponse {
  agent_id: string;
  heartbeat_interval_seconds: number;
  task_report_interval_seconds: number;
}

export interface TaskDispatchRequest {
  task_id: string;
  logical_task_key: string;
  strategy_id: string;
  config_snapshot_id: string;
  scan_start_time: string;
  collect_id: string;
  load_type: string;
  encoding: string;
  output_delimiter: string;
  timeout_seconds: number;
  callback_base_url: string;
}

export interface TaskDispatchResponse {
  task_id: string;
  accepted: boolean;
  agent_task_state: string;
  reason: string | null;
}

export interface ResultRow {
  table_name: string;
  data_time: string;
  row_count: number;
  success: number;
  collect_time: string;
}

export interface TaskResultReport {
  task_id: string;
  agent_id: string;
  status: string;
  result_rows: ResultRow[];
}

export interface DailyGrid {
  day: string;
  time_slots: string[];
  rows: TableGridRow[];
}

export interface TableGridRow {
  table_name: string;
  cells: GridCell[];
}

export interface GridCell {
  data_time: string;
  value: number | null;
  color: 'green' | 'yellow' | 'red' | 'gray';
  status: 'ok' | 'empty' | 'failed' | 'missing';
}

export interface GridQuery {
  strategy_id: string;
  day: string;
  interval_minutes?: number;
}

export type AgentStatus = 'ONLINE' | 'OFFLINE' | 'UNKNOWN';

export interface AgentInfo {
  agent_id: string;
  agent_name: string;
  host: string;
  port: number;
  version: string;
  capabilities: AgentCapabilities;
  status: AgentStatus;
  registered_at: string;
  last_heartbeat_at: string | null;
}

export interface ConfigUpdateRequest {
  snapshot_id: string;
  content_hash: string;
}

export interface DataCollectorUnit {
  id: number;
  unit_name: string;
  config_name: string;
  config_version: string;
  table_names: string[];
  agent_ids: string[];
  data_interval_seconds: number;
  collector_interval: number;
  task_timeout_seconds: number;
  source_type: string;
  file_encoding: string;
  remote_pattern: string;
  host: string;
  port: number;
  username: string;
  password: string;
  connect_retry: number;
  download_retry: number;
  download_parallel: number;
  retry_interval_secs: number;
  connect_timeout_secs: number;
  read_timeout_secs: number;
  cache_retention_days: number;
  load_type: string;
  output_delimiter: string;
  db_host: string;
  db_port: number;
  db_user: string;
  db_password: string;
  db_database: string;
  db_table_name_case: string;
  created_at: string;
  updated_at: string;
}

export interface DataCollectorUnitSaveRequest {
  unit_name: string;
  config_name: string;
  table_names: string;
  agent_ids: string;
  data_interval_seconds?: number;
  collector_interval?: number;
  task_timeout_seconds?: number;
  source_type?: string;
  file_encoding?: string;
  remote_pattern?: string;
  host?: string;
  port?: number;
  username?: string;
  password?: string;
  connect_retry?: number;
  download_retry?: number;
  download_parallel?: number;
  retry_interval_secs?: number;
  connect_timeout_secs?: number;
  read_timeout_secs?: number;
  cache_retention_days?: number;
  load_type?: string;
  output_delimiter?: string;
  db_host?: string;
  db_port?: number;
  db_user?: string;
  db_password?: string;
  db_database?: string;
  db_table_name_case?: string;
}

export interface NextIdResponse {
  id: number;
}

export interface ConfigNameItem {
  name: string;
  version: string;
}

export interface ConfigNamesResponse {
  config_names: ConfigNameItem[];
}

export interface TablesResponse {
  tables: string[];
}

export interface CollectionStrategy {
  id: number;
  collector_name: string;
  collector_id: number;
  table_name: string;
  status: string;
  cron_expression: string;
  collect_interval: number;
  data_interval: number;
  data_start_time: string | null;
  data_end_time: string | null;
  execute_time: string | null;
  agent_ids: string[];
  strategy_type: string;
  created_at: string;
  updated_at: string;
}

export interface CollectionStrategyCreateRequest {
  collector_id: number;
  collector_name: string;
  table_names: string[];
  cron_expression?: string;
  collect_interval: number;
  data_interval: number;
  data_start_time?: string;
  data_end_time?: string;
  execute_time?: string;
  agent_ids: string;
  strategy_type: string;
}

export interface CollectionStrategyUpdateRequest {
  cron_expression?: string;
  collect_interval?: number;
  data_interval?: number;
  data_start_time?: string;
  data_end_time?: string;
  execute_time?: string;
  agent_ids?: string;
  status?: string;
}

export interface BatchStatusRequest {
  ids: number[];
}
