import { Table } from 'antd';
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
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 100,
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
      render: (v: string | null) => v ?? '-',
    },
  ];

  return (
    <div>
      <h2 style={{ marginBottom: 16 }}>Agent 管理</h2>
      <Table
        dataSource={agents}
        columns={columns}
        loading={isLoading}
        rowKey="agent_id"
        pagination={false}
      />
    </div>
  );
}
