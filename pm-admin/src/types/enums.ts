export const TaskStatus = {
  CREATED: 'CREATED',
  DISPATCHING: 'DISPATCHING',
  ACCEPTED: 'ACCEPTED',
  RUNNING: 'RUNNING',
  SUCCEEDED: 'SUCCEEDED',
  FAILED: 'FAILED',
  TIMEOUT: 'TIMEOUT',
  CANCEL_REQUESTED: 'CANCEL_REQUESTED',
  CANCELLED: 'CANCELLED',
} as const;

export type TaskStatus = (typeof TaskStatus)[keyof typeof TaskStatus];

export const GRID_COLORS: Record<string, { color: string; label: string }> = {
  green: { color: '#52c41a', label: '正常' },
  yellow: { color: '#faad14', label: '空数据' },
  red: { color: '#ff4d4f', label: '失败' },
  gray: { color: '#d9d9d9', label: '缺失' },
};
