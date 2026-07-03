import { Table, Card } from 'antd';
import { useAgents } from '../../api/hooks';
import StatusTag from '../../components/StatusTag';

export default function AgentsPage() {
  const { data: agents, isLoading } = useAgents();

  const columns = [
    {
      title: 'Agent ID',
      dataIndex: 'agent_id',
      key: 'agent_id',
      ellipsis: true,
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '名称',
      dataIndex: 'agent_name',
      key: 'name',
    },
    {
      title: '主机',
      dataIndex: 'host',
      key: 'host',
    },
    {
      title: '端口',
      dataIndex: 'port',
      key: 'port',
      width: 80,
      render: (v: number) => <span className="mono">{v}</span>,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 110,
      render: (s: string) => <StatusTag status={s} />,
    },
    {
      title: '版本',
      dataIndex: 'version',
      key: 'version',
      width: 100,
    },
    {
      title: '最后心跳',
      dataIndex: 'last_heartbeat',
      key: 'heartbeat',
      render: (v: string | null) => v ?? <span style={{ color: '#94a3b8' }}>-</span>,
    },
  ];

  return (
    <div>
      <div className="page-header">
        <h2>Agent 管理</h2>
        <p>查看在线采集 Agent 及其健康状态</p>
      </div>
      <Card className="content-card" styles={{ body: { padding: 0 } }}>
        <Table
          className="data-table"
          dataSource={agents}
          columns={columns}
          loading={isLoading}
          rowKey="agent_id"
          pagination={false}
        />
      </Card>
    </div>
  );
}
