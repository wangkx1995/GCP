import { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Table, Card, Button, message, Popconfirm, Empty, Tooltip } from 'antd';
import { PlusOutlined, DeleteOutlined, EditOutlined } from '@ant-design/icons';
import { useDataCollectorUnits, useDeleteDataCollectorUnit } from '../../api/hooks';
import type { DataCollectorUnit } from '../../types/api';

export default function DataCollectorUnitsPage() {
  const navigate = useNavigate();
  const { data: units, isLoading } = useDataCollectorUnits();
  const deleteMutation = useDeleteDataCollectorUnit();
  const [deletingId, setDeletingId] = useState<number | null>(null);

  const handleDelete = useCallback(async (id: number) => {
    setDeletingId(id);
    try {
      await deleteMutation.mutateAsync(id);
      message.success('删除成功');
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    } finally {
      setDeletingId(null);
    }
  }, [deleteMutation]);

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '单元名称', dataIndex: 'unit_name', key: 'unit_name' },
    { title: '适配器名称', dataIndex: 'config_name', key: 'config_name' },
    { title: '适配器版本', dataIndex: 'config_version', key: 'config_version' },
    { title: '可采集表', key: 'tables', render: (_: unknown, r: DataCollectorUnit) => r.table_names.join(', ') },
    { title: '采集机', key: 'agents', render: (_: unknown, r: DataCollectorUnit) => r.agent_ids.join(', ') },
    { title: '数据源', dataIndex: 'source_type', key: 'source_type' },
    {
      title: '操作', key: 'action', width: 120,
      render: (_: unknown, record: DataCollectorUnit) => (
        <span onClick={e => e.stopPropagation()}>
          <Tooltip title="编辑">
            <Button type="link" size="small" icon={<EditOutlined />} aria-label="编辑" onClick={() => navigate(`/data-collector-units/${record.id}/edit`)} />
          </Tooltip>
          <Popconfirm title="确认删除?" onConfirm={() => handleDelete(record.id)}>
            <Tooltip title="删除">
              <Button danger type="link" size="small" icon={<DeleteOutlined />} aria-label="删除" loading={deletingId === record.id} />
            </Tooltip>
          </Popconfirm>
        </span>
      ),
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>采集单元管理</h2>
        <p>管理采集单元，绑定适配器、采集机、数据源和调度配置</p>
      </div>

      <div className="page-body">
      <Card
        title="采集单元列表"
        className="content-card"
        styles={{ body: { padding: 0 } }}
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={() => navigate('/data-collector-units/create')}>
            新建
          </Button>
        }
      >
        <Table<DataCollectorUnit>
          className="data-table"
          rowKey="id"
          dataSource={units}
          columns={columns}
          loading={isLoading}
          pagination={false}
          size="small"
          locale={{ emptyText: <Empty description="暂无采集单元，点击右上角新建" /> }}
          scroll={{ x: 'max-content' }}
          onRow={(record) => ({
            onClick: () => navigate(`/data-collector-units/${record.id}/edit`),
            style: { cursor: 'pointer' },
          })}
        />
      </Card>
      </div>
    </div>
  );
}