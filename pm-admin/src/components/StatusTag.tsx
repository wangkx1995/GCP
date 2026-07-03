import { Tag } from 'antd';

const STATUS_CONFIG: Record<string, { color: string; dot: string; label: string }> = {
  ONLINE: { color: 'green', dot: '#10b981', label: '在线' },
  OFFLINE: { color: 'default', dot: '#94a3b8', label: '离线' },
  UNKNOWN: { color: 'default', dot: '#94a3b8', label: '未知' },
  SUCCEEDED: { color: 'green', dot: '#10b981', label: '成功' },
  FAILED: { color: 'red', dot: '#ef4444', label: '失败' },
  RUNNING: { color: 'blue', dot: '#0891b2', label: '运行中' },
  TIMEOUT: { color: 'orange', dot: '#f59e0b', label: '超时' },
  CANCELLED: { color: 'default', dot: '#94a3b8', label: '已取消' },
};

interface Props {
  status: string;
}

export default function StatusTag({ status }: Props) {
  const config = STATUS_CONFIG[status] ?? { color: 'default', dot: '#94a3b8', label: status };
  return (
    <Tag color={config.color} style={{ borderRadius: 4, paddingInline: 8 }}>
      <span className="status-dot" style={{ background: config.dot }} />
      {config.label}
    </Tag>
  );
}
