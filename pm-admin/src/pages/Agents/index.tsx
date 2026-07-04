import { Table, Tag, Card, Badge, Typography, Space } from 'antd';
import { CloudServerOutlined, WifiOutlined } from '@ant-design/icons';
import { useAgents } from '../../api/hooks';
import type { AgentInfo, AgentStatus } from '../../types/api';

const { Text } = Typography;

const statusConfig: Record<AgentStatus, { color: string; label: string }> = {
  ONLINE: { color: '#22C55E', label: '在线' },
  OFFLINE: { color: '#EF4444', label: '离线' },
  UNKNOWN: { color: '#94A3B8', label: '未知' },
};

const columns = [
  {
    title: '采集机',
    dataIndex: 'agent_name',
    key: 'agent_name',
    render: (_: string, record: AgentInfo) => (
      <Space>
        <CloudServerOutlined style={{ fontSize: 18, color: '#64748B' }} />
        <div>
          <div style={{ fontWeight: 600, lineHeight: 1.4 }}>{record.agent_name}</div>
          <Text type="secondary" className="mono" style={{ fontSize: 12 }}>
            {record.agent_id}
          </Text>
        </div>
      </Space>
    ),
  },
  {
    title: '地址',
    key: 'addr',
    render: (_: string, record: AgentInfo) => (
      <Space>
        <WifiOutlined style={{ color: '#94A3B8' }} />
        <span className="mono">{record.host}:{record.port}</span>
      </Space>
    ),
  },
  {
    title: '版本',
    dataIndex: 'version',
    key: 'version',
    render: (v: string) => <Tag className="mono">{v}</Tag>,
  },
  {
    title: '能力',
    key: 'capabilities',
    render: (_: string, record: AgentInfo) => (
      <Space size={4} wrap>
        {record.capabilities.can_collect && <Tag color="blue">采集</Tag>}
        {record.capabilities.can_parse && <Tag color="green">解析</Tag>}
        {record.capabilities.can_load && <Tag color="orange">入库</Tag>}
      </Space>
    ),
  },
  {
    title: '状态',
    dataIndex: 'status',
    key: 'status',
    render: (s: AgentStatus) => {
      const cfg = statusConfig[s];
      return (
        <Space>
          <Badge color={cfg.color} />
          {cfg.label}
        </Space>
      );
    },
  },
  {
    title: '注册时间',
    dataIndex: 'registered_at',
    key: 'registered_at',
    render: (v: string) => <span className="mono" style={{ fontSize: 13 }}>{v}</span>,
  },
  {
    title: '最后心跳',
    dataIndex: 'last_heartbeat_at',
    key: 'last_heartbeat_at',
    render: (v: string | null) =>
      v ? (
        <span className="mono" style={{ fontSize: 13 }}>{v}</span>
      ) : (
        <Text type="secondary">—</Text>
      ),
  },
];

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgents();

  return (
    <div>
      <div className="page-header">
        <h2>采集机管理</h2>
        <p>已注册的采集机（自动注册，无需手动添加）</p>
      </div>

      <Card className="content-card" styles={{ body: { padding: 0 } }}>
        <Table<AgentInfo>
          className="data-table"
          rowKey="agent_id"
          dataSource={agents}
          columns={columns}
          loading={isLoading}
          pagination={false}
        />
      </Card>
    </div>
  );
}
