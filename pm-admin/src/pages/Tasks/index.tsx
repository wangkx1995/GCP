import { Table } from 'antd';
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
      <h2 style={{ marginBottom: 16 }}>任务列表</h2>
      <Table
        dataSource={tasks}
        columns={columns}
        loading={isLoading}
        rowKey="task_id"
        pagination={false}
      />
    </div>
  );
}
