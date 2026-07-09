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
  result: string;
}

export interface TaskDispatchRequest {
  task_id: string;
  logical_task_key: string;
  strategy_id: string;
  config_snapshot_id: string;
  scan_start_time: string;
  collector_name: string;
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
  color: 'green' | 'yellow' | 'red' | 'gray' | 'none';
  status: 'ok' | 'empty' | 'failed' | 'missing' | 'future';
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
  agent_alias: string;
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
  id: string;
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
  original_id?: string;
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
  id: string;
  collector_name: string;
  collector_id: string;
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
  delay_period?: number;
  created_at: string;
  updated_at: string;
}

export interface CollectionStrategyCreateRequest {
  collector_id: string;
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
  delay_period?: number;
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
  delay_period?: number;
}

export interface BatchStatusRequest {
  ids: string[];
}

export interface AgentInfoRow {
  agent_id: string;
  agent_name: string;
  agent_ip: string;
  port: number;
  version: string;
  cpu_total?: string;
  memory_total?: number;
  disk_total?: number;
  heartbeat_interval?: number;
  time_stamp?: string;
  description?: string;
  max_thread_num?: number;
  agent_isuse_flag: number;
  fact_memory_total?: number;
  agent_alias?: string;
  is_core: number;
  agent_power?: number;
  host_load_limit?: number;
  registered_at: string;
  current_status?: string;
  cpu_load?: number;
  memory_load?: number;
  disk_load?: number;
  current_thread_num?: number;
  last_heartbeat_time?: string;
}

export interface AgentStatusRow {
  agent_id: string;
  agent_name: string;
  status: string;
  cpu_load?: number;
  memory_load?: number;
  disk_load?: number;
  heartbeat_time: string;
  thread_num?: number;
  agent_alias?: string;
  agent_power?: number;
  new_task_count: number;
  active_task_count: number;
}

export interface AgentStatusHisRow {
  agent_id: string;
  cpu_load?: number;
  memory_load?: number;
  disk_load?: number;
  heartbeat_time: string;
  thread_num?: number;
  insert_time?: string;
}

export interface AgentGroupRow {
  group_id: string;
  group_name: string;
  agent_ids: string;
  description?: string;
  time_stamp?: string;
}

export interface UpdateAgentRequest {
  agent_alias?: string;
  agent_isuse_flag?: number;
  agent_power?: number;
  host_load_limit?: number;
  description?: string;
}

export interface CreateGroupRequest {
  group_name: string;
  agent_ids: string;
  description?: string;
}

export interface UpdateGroupRequest {
  group_name: string;
  agent_ids: string;
  description?: string;
}
