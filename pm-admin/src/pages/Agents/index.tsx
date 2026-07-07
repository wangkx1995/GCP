import { Table, Tag, Card, Typography, Space, Switch } from 'antd';
import { CloudServerOutlined, WifiOutlined } from '@ant-design/icons';
import { useAgentList } from '../../api/hooks';
import type { AgentInfoRow } from '../../types/api';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { updateAgent } from '../../api/agents';

const { Text } = Typography;

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgentList();
  const queryClient = useQueryClient();

  const toggleIsuse = useMutation({
    mutationFn: ({ id, flag }: { id: number; flag: number }) =>
      updateAgent(id, { agent_isuse_flag: flag }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['agent-list'] }),
  });

  const columns = [
    {
      title: 'ID',
      dataIndex: 'agent_id',
      key: 'agent_id',
      width: 160,
      render: (v: number) => <span className="mono">{v.toLocaleString()}</span>,
    },
    {
      title: '名称',
      key: 'name',
      width: 180,
      render: (_: unknown, record: AgentInfoRow) => (
        <Space>
          <CloudServerOutlined style={{ fontSize: 18, color: '#64748B' }} />
          <div>
            <div style={{ fontWeight: 600 }}>{record.agent_name}</div>
            {record.agent_alias && (
              <Text type="secondary" style={{ fontSize: 12 }}>{record.agent_alias}</Text>
            )}
          </div>
        </Space>
      ),
    },
    {
      title: '别名',
      dataIndex: 'agent_alias',
      key: 'agent_alias',
      width: 120,
      render: (v: string | undefined) => v || '—',
    },
    {
      title: 'IP',
      dataIndex: 'agent_ip',
      key: 'agent_ip',
      width: 150,
      render: (v: string) => (
        <Space>
          <WifiOutlined style={{ color: '#94A3B8' }} />
          <span className="mono">{v}</span>
        </Space>
      ),
    },
    {
      title: '端口',
      dataIndex: 'port',
      key: 'port',
      width: 80,
      render: (v: number) => <span className="mono">{v}</span>,
    },
    {
      title: '版本',
      dataIndex: 'version',
      key: 'version',
      width: 100,
      render: (v: string) => <Tag className="mono">{v}</Tag>,
    },
    {
      title: 'CPU',
      dataIndex: 'cpu_total',
      key: 'cpu_total',
      width: 100,
      render: (v: string | undefined) => v || '—',
    },
    {
      title: '内存(MB)',
      key: 'memory',
      width: 120,
      render: (_: unknown, r: AgentInfoRow) => {
        const mem = r.fact_memory_total || r.memory_total;
        if (mem) return (mem / (1024 * 1024)).toFixed(0);
        return '—';
      },
    },
    {
      title: '磁盘(GB)',
      dataIndex: 'disk_total',
      key: 'disk_total',
      width: 110,
      render: (v: number | undefined) => v ? (v / (1024 * 1024 * 1024)).toFixed(1) : '—',
    },
    {
      title: '类型',
      dataIndex: 'is_core',
      key: 'is_core',
      width: 100,
      render: (v: number) => v === 1 ? <Tag color="red">核心机</Tag> : <Tag>采集机</Tag>,
    },
    {
      title: '权重',
      dataIndex: 'agent_power',
      key: 'agent_power',
      width: 80,
      render: (v: number | undefined) => v ?? '—',
    },
    {
      title: '负载上限',
      dataIndex: 'host_load_limit',
      key: 'host_load_limit',
      width: 100,
      render: (v: number | undefined) => v != null ? `${v}%` : '—',
    },
    {
      title: '心跳间隔',
      dataIndex: 'heartbeat_interval',
      key: 'heartbeat_interval',
      width: 100,
      render: (v: number | undefined) => v ? `${v}s` : '—',
    },
    {
      title: '启用',
      dataIndex: 'agent_isuse_flag',
      key: 'agent_isuse_flag',
      width: 80,
      render: (flag: number, record: AgentInfoRow) => (
        <Switch
          checked={flag === 1}
          onChange={(checked) =>
            toggleIsuse.mutate({ id: record.agent_id, flag: checked ? 1 : 0 })
          }
        />
      ),
    },
    {
      title: '描述',
      dataIndex: 'description',
      key: 'description',
      width: 200,
      ellipsis: true,
      render: (v: string | undefined) => v || '—',
    },
    {
      title: '注册时间',
      dataIndex: 'registered_at',
      key: 'registered_at',
      width: 180,
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '更新时间',
      dataIndex: 'time_stamp',
      key: 'time_stamp',
      width: 180,
      render: (v: string | undefined) => v ? <span className="mono">{v}</span> : '—',
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>采集机信息</h2>
        <p>已注册的采集机节点（启动时自动注册）</p>
      </div>
      <div className="page-body">
        <Card className="content-card" styles={{ body: { padding: 0 } }}>
          <div className="table-scroll-wrap">
            <Table<AgentInfoRow>
              className="data-table"
              rowKey="agent_id"
              dataSource={agents}
              columns={columns}
              loading={isLoading}
              pagination={false}
              scroll={{ x: 'max-content', y: 'var(--table-scroll-y)' }}
            />
          </div>
        </Card>
      </div>
    </div>
  );
}
