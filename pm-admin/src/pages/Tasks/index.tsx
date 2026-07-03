import { Table, Card } from 'antd';
import { useTasks } from '../../api/hooks';
import StatusTag from '../../components/StatusTag';

export default function TasksPage() {
  const { data: tasks, isLoading } = useTasks();

  const columns = [
    {
      title: '任务 ID',
      dataIndex: 'task_id',
      key: 'task_id',
      ellipsis: true,
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '策略 ID',
      dataIndex: 'strategy_id',
      key: 'strategy',
      ellipsis: true,
    },
    {
      title: '状态',
      dataIndex: 'status',
      key: 'status',
      width: 120,
      render: (s: string) => <StatusTag status={s} />,
    },
    {
      title: '创建时间',
      dataIndex: 'created_at',
      key: 'created',
      width: 180,
    },
    {
      title: '更新时间',
      dataIndex: 'updated_at',
      key: 'updated',
      width: 180,
    },
  ];

  return (
    <div>
      <div className="page-header">
        <h2>任务列表</h2>
        <p>查看采集任务执行历史与状态</p>
      </div>
      <Card className="content-card" styles={{ body: { padding: 0 } }}>
        <Table
          className="data-table"
          dataSource={tasks}
          columns={columns}
          loading={isLoading}
          rowKey="task_id"
          pagination={false}
        />
      </Card>
    </div>
  );
}
