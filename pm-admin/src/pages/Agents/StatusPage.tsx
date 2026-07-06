import { useState } from 'react';
import { Table, Card, Badge, Space, Typography, Switch } from 'antd';
import { CloudServerOutlined } from '@ant-design/icons';
import { useAgentStatusList } from '../../api/hooks';
import type { AgentStatusRow } from '../../types/api';

const { Text } = Typography;

const statusConfig: Record<string, { color: string; label: string }> = {
  ONLINE: { color: '#22C55E', label: '在线' },
  OFFLINE: { color: '#EF4444', label: '离线' },
  UNKNOWN: { color: '#94A3B8', label: '未知' },
};

const columns = [
  {
    title: 'Agent ID',
    dataIndex: 'agent_id',
    key: 'agent_id',
    render: (v: number) => <span className="mono">{v}</span>,
  },
  {
    title: '名称',
    dataIndex: 'agent_name',
    key: 'agent_name',
    render: (v: string) => (
      <Space>
        <CloudServerOutlined style={{ color: '#64748B' }} />
        <span>{v}</span>
      </Space>
    ),
  },
  {
    title: '状态',
    dataIndex: 'status',
    key: 'status',
    render: (s: string) => {
      const cfg = statusConfig[s] ?? { color: '#94A3B8', label: s };
      return (
        <Space>
          <Badge color={cfg.color} />
          {cfg.label}
        </Space>
      );
    },
  },
  {
    title: 'CPU 负载',
    dataIndex: 'cpu_load',
    key: 'cpu_load',
    render: (v: number | null) =>
      v != null ? <span className="mono">{v.toFixed(1)}%</span> : <Text type="secondary">—</Text>,
  },
  {
    title: '内存负载',
    dataIndex: 'memory_load',
    key: 'memory_load',
    render: (v: number | null) =>
      v != null ? <span className="mono">{v.toFixed(1)}%</span> : <Text type="secondary">—</Text>,
  },
  {
    title: '磁盘负载',
    dataIndex: 'disk_load',
    key: 'disk_load',
    render: (v: number | null) =>
      v != null ? <span className="mono">{v.toFixed(1)}%</span> : <Text type="secondary">—</Text>,
  },
  {
    title: '线程数',
    dataIndex: 'thread_num',
    key: 'thread_num',
    render: (v: number | null | undefined) =>
      v != null ? <span className="mono">{v}</span> : <Text type="secondary">—</Text>,
  },
  {
    title: '最后心跳',
    dataIndex: 'heartbeat_time',
    key: 'heartbeat_time',
    render: (v: string) => <span className="mono">{v}</span>,
  },
];

export default function AgentStatusPage() {
  const [autoRefresh, setAutoRefresh] = useState(true);
  const { data: list, isLoading } = useAgentStatusList();

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>实时状态</h2>
        <p>采集机当前负载信息（每 30 秒自动刷新）</p>
      </div>

      <div className="page-body">
        <Card
          className="content-card"
          styles={{ body: { padding: 0 } }}
          extra={
            <Space>
              <Text type="secondary" style={{ fontSize: 13 }}>自动刷新</Text>
              <Switch size="small" checked={autoRefresh} onChange={setAutoRefresh} />
            </Space>
          }
        >
          <Table<AgentStatusRow>
            className="data-table"
            rowKey="agent_id"
            dataSource={list}
            columns={columns}
            loading={isLoading}
            pagination={false}
          />
        </Card>
      </div>
    </div>
  );
}
