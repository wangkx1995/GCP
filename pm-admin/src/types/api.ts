export interface ConfigSnapshotMeta {
  config_snapshot_id: string;
  content_hash: string;
  version_label: string | null;
  is_active: boolean;
  file_count: number;
  created_at: string;
  activated_at: string | null;
}

export interface UploadSuccessResponse {
  valid: true;
  config_snapshot_id: string;
  content_hash: string;
  file_count: number;
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

export interface ConfigUpdateRequest {
  snapshot_id: string;
  content_hash: string;
}
