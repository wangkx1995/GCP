import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Table, Card, Button, message, Popconfirm, Tag, Space, Empty, Tooltip } from 'antd';
import { EditOutlined, PauseCircleOutlined, PlayCircleOutlined } from '@ant-design/icons';
import { useStrategies, useBatchSuspend, useBatchActivate } from '../../api/hooks';
import type { CollectionStrategy } from '../../types/api';
import type { ColumnsType } from 'antd/es/table';

export default function StrategyInfoPage() {
  const navigate = useNavigate();
  const { data: strategies, isLoading, refetch } = useStrategies();
  const suspendMutation = useBatchSuspend();
  const activateMutation = useBatchActivate();
  const [selectedIds, setSelectedIds] = useState<number[]>([]);

  const handleBatchSuspend = useCallback(async () => {
    if (selectedIds.length === 0) return;
    try {
      await suspendMutation.mutateAsync(selectedIds);
      message.success(`已挂起 ${selectedIds.length} 条`);
      setSelectedIds([]);
      refetch();
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [selectedIds, suspendMutation, refetch]);

  const handleBatchActivate = useCallback(async () => {
    if (selectedIds.length === 0) return;
    try {
      await activateMutation.mutateAsync(selectedIds);
      message.success(`已激活 ${selectedIds.length} 条`);
      setSelectedIds([]);
      refetch();
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [selectedIds, activateMutation, refetch]);

  function fmt(v: string | null | undefined): string {
    return v ?? '—';
  }

  const columns: ColumnsType<CollectionStrategy> = [
    { title: '策略Id', dataIndex: 'id', key: 'id', width: 70 },
    { title: '采集单元', dataIndex: 'collector_name', key: 'collector_name' },
    { title: '表名', dataIndex: 'table_name', key: 'table_name' },
    {
      title: '类型', dataIndex: 'strategy_type', key: 'strategy_type', width: 80,
      render: (v: string) => v === 'immediate' ? <Tag color="blue">及时</Tag> : <Tag color="green">周期</Tag>,
    },
    {
      title: '采集周期(秒)', dataIndex: 'collect_interval', key: 'collect_interval', width: 100,
      render: (v: number) => fmt(v?.toString()),
    },
    {
      title: '数据周期(秒)', dataIndex: 'data_interval', key: 'data_interval', width: 100,
      render: (v: number) => fmt(v?.toString()),
    },
    { title: 'Crontab', dataIndex: 'cron_expression', key: 'cron_expression', render: v => fmt(v) },
    {
      title: '开始时间', dataIndex: 'data_start_time', key: 'data_start_time', width: 150,
      render: v => fmt(v),
    },
    {
      title: '结束时间', dataIndex: 'data_end_time', key: 'data_end_time', width: 150,
      render: v => fmt(v),
    },
    {
      title: '执行时间', dataIndex: 'execute_time', key: 'execute_time', width: 150,
      render: v => fmt(v),
    },
    {
      title: '采集机', key: 'agents', width: 160,
      render: (_: unknown, r: CollectionStrategy) => r.agent_ids.join(', '),
    },
    {
      title: '状态', dataIndex: 'status', key: 'status', width: 80,
      render: (v: string) => v === '可用'
        ? <Tag color="success">可用</Tag>
        : <Tag color="default">挂起</Tag>,
    },

    {
      title: '操作', key: 'action', width: 140, fixed: 'right',
      render: (_: unknown, record: CollectionStrategy) => (
        <span onClick={e => e.stopPropagation()}>
          <Tooltip title="编辑">
            <Button type="link" size="small" icon={<EditOutlined />} aria-label="编辑"
              onClick={() => navigate(`/strategy-dispatch/${record.strategy_type}/${record.id}/edit`)} />
          </Tooltip>
          {record.status === '可用' ? (
            <Popconfirm title="确认挂起?" onConfirm={async () => {
              await suspendMutation.mutateAsync([record.id]);
              message.success('已挂起');
              refetch();
            }}>
              <Tooltip title="挂起">
                <Button type="link" size="small" icon={<PauseCircleOutlined />} aria-label="挂起" />
              </Tooltip>
            </Popconfirm>
          ) : (
            <Popconfirm title="确认激活?" onConfirm={async () => {
              await activateMutation.mutateAsync([record.id]);
              message.success('已激活');
              refetch();
            }}>
              <Tooltip title="激活">
                <Button type="link" size="small" icon={<PlayCircleOutlined />} aria-label="激活" />
              </Tooltip>
            </Popconfirm>
          )}
        </span>
      ),
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>采集策略信息</h2>
        <p>查看和管理所有采集策略</p>
      </div>

      <div className="page-body">
      <Card
        className="content-card"
        styles={{ body: { padding: 0 } }}
        extra={
          <Space>
            {selectedIds.length > 0 && (
              <>
                <Button icon={<PauseCircleOutlined />} onClick={handleBatchSuspend} loading={suspendMutation.isPending}>
                  批量挂起 ({selectedIds.length})
                </Button>
                <Button icon={<PlayCircleOutlined />} onClick={handleBatchActivate} loading={activateMutation.isPending}>
                  批量激活 ({selectedIds.length})
                </Button>
              </>
            )}
          </Space>
        }
      >
        <Table<CollectionStrategy>
          className="data-table"
          rowKey="id"
          dataSource={strategies}
          columns={columns}
          loading={isLoading}
          pagination={false}
          size="small"
          scroll={{ x: 'max-content' }}
          locale={{ emptyText: <Empty description="暂无策略数据" /> }}
          rowSelection={{
            selectedRowKeys: selectedIds,
            onChange: (keys) => setSelectedIds(keys as number[]),
          }}
          onRow={(record) => ({
            onClick: () => navigate(`/strategy-dispatch/${record.strategy_type}/${record.id}/edit`),
            style: { cursor: 'pointer' },
          })}
        />
      </Card>
      </div>
    </div>
  );
}
