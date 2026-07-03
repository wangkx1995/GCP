import { Tag } from 'antd';

const STATUS_CONFIG: Record<string, { color: string; label: string }> = {
  ONLINE: { color: 'green', label: '在线' },
  OFFLINE: { color: 'default', label: '离线' },
  UNKNOWN: { color: 'default', label: '未知' },
  SUCCEEDED: { color: 'green', label: '成功' },
  FAILED: { color: 'red', label: '失败' },
  RUNNING: { color: 'blue', label: '运行中' },
  TIMEOUT: { color: 'orange', label: '超时' },
  CANCELLED: { color: 'default', label: '已取消' },
};

interface Props {
  status: string;
}

export default function StatusTag({ status }: Props) {
  const config = STATUS_CONFIG[status] ?? { color: 'default', label: status };
  return <Tag color={config.color}>{config.label}</Tag>;
}
