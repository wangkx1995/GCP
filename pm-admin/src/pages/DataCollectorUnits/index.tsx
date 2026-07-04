import { useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Table, Card, Button, message, Popconfirm } from 'antd';
import { PlusOutlined, DeleteOutlined, EditOutlined } from '@ant-design/icons';
import { useDataCollectorUnits, useDeleteDataCollectorUnit } from '../../api/hooks';
import type { DataCollectorUnit } from '../../types/api';

function tryParseJson(val: string, fallback: string[]) {
  try { return JSON.parse(val); } catch { return fallback; }
}

export default function DataCollectorUnitsPage() {
  const navigate = useNavigate();
  const { data: units, isLoading } = useDataCollectorUnits();
  const deleteMutation = useDeleteDataCollectorUnit();

  const handleDelete = useCallback(async (id: number) => {
    try {
      await deleteMutation.mutateAsync(id);
      message.success('删除成功');
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [deleteMutation]);

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '单元名称', dataIndex: 'unit_name', key: 'unit_name' },
    { title: '适配器名称', dataIndex: 'config_name', key: 'config_name' },
    { title: '适配器版本', dataIndex: 'config_version', key: 'config_version' },
    { title: '采集表', key: 'tables', render: (_: unknown, r: DataCollectorUnit) => tryParseJson(r.table_names, []).join(', ') },
    { title: '采集机数', key: 'agent_count', render: (_: unknown, r: DataCollectorUnit) => tryParseJson(r.agent_ids, []).length },
    { title: '数据源', dataIndex: 'source_type', key: 'source_type' },
    {
      title: '操作', key: 'action', width: 120,
      render: (_: unknown, record: DataCollectorUnit) => (
        <span onClick={e => e.stopPropagation()}>
          <Button type="link" size="small" icon={<EditOutlined />} onClick={() => navigate(`/data-collector-units/${record.id}`)} />
          <Popconfirm title="确认删除?" onConfirm={() => handleDelete(record.id)}>
            <Button danger type="link" size="small" icon={<DeleteOutlined />} loading={deleteMutation.isPending} />
          </Popconfirm>
        </span>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <h2>采集单元管理</h2>
        <p>管理采集单元，绑定适配器、采集机、数据源和调度配置</p>
      </div>

      <Card
        title="采集单元列表"
        className="content-card"
        styles={{ body: { padding: 0 } }}
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={() => navigate('/data-collector-units/new')}>
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
          scroll={{ x: 'max-content' }}
          onRow={(record) => ({
            onClick: () => navigate(`/data-collector-units/${record.id}`),
            style: { cursor: 'pointer' },
          })}
        />
      </Card>
    </div>
  );
}