import { useCallback, useState } from 'react';
import { Table, Card, Button, Modal, Form, Input, Select, Popconfirm, message } from 'antd';
import { PlusOutlined, DeleteOutlined, EditOutlined } from '@ant-design/icons';
import type { AgentGroupRow } from '../../types/api';
import { useAgentGroupList, useCreateAgentGroup, useUpdateAgentGroup, useDeleteAgentGroup, useAgents } from '../../api/hooks';

export default function AgentGroupsPage() {
  const { data: groups, isLoading } = useAgentGroupList();
  const { data: agents } = useAgents();
  const createMutation = useCreateAgentGroup();
  const updateMutation = useUpdateAgentGroup();
  const deleteMutation = useDeleteAgentGroup();
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<AgentGroupRow | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [form] = Form.useForm();

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    setModalOpen(true);
  };

  const openEdit = (record: AgentGroupRow) => {
    setEditing(record);
    form.setFieldsValue({
      ...record,
      agent_ids: record.agent_ids ? record.agent_ids.split(',').filter(Boolean) : [],
    });
    setModalOpen(true);
  };

  const handleOk = async () => {
    const values = await form.validateFields();
    const payload = {
      group_name: values.group_name,
      agent_ids: (values.agent_ids ?? []).join(','),
      description: values.description,
    };
    try {
      if (editing) {
        await updateMutation.mutateAsync({ id: editing.group_id, data: payload });
        message.success('更新成功');
      } else {
        await createMutation.mutateAsync(payload);
        message.success('创建成功');
      }
      setModalOpen(false);
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  };

  const handleDelete = useCallback(async (id: string) => {
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

  const agentOptions = agents?.map(a => ({
    value: a.agent_id,
    label: a.agent_alias,
  })) ?? [];

  const columns = [
    { title: '组名', dataIndex: 'group_name', key: 'group_name' },
    {
      title: '采集机',
      key: 'agent_names',
      render: (_: unknown, r: AgentGroupRow) => {
        if (!r.agent_ids) return '-';
        const agentMap = new Map(agents?.map(a => [String(a.agent_id), a.agent_alias]) ?? []);
        return r.agent_ids.split(',').map(id => {
          const name = agentMap.get(id);
          return name ?? id;
        }).join(', ');
      },
    },
    { title: '描述', dataIndex: 'description', key: 'description', ellipsis: true },
    { title: '时间', dataIndex: 'time_stamp', key: 'time_stamp', render: (v: string) => <span className="mono">{v}</span> },
    {
      title: '操作', key: 'action', width: 120,
      render: (_: unknown, record: AgentGroupRow) => (
        <span onClick={e => e.stopPropagation()}>
          <Button type="link" size="small" icon={<EditOutlined />} onClick={() => openEdit(record)}>编辑</Button>
          <Popconfirm title="确认删除?" onConfirm={() => handleDelete(record.group_id)}>
            <Button danger type="link" size="small" icon={<DeleteOutlined />} loading={deletingId === record.group_id}>删除</Button>
          </Popconfirm>
        </span>
      ),
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>采集机组</h2>
        <p>管理采集机组，用于分组调度</p>
      </div>

      <div className="page-body">
        <Card
          title="机组列表"
          className="content-card"
          styles={{ body: { padding: 0 } }}
          extra={
            <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>新建组</Button>
          }
        >
          <div className="table-scroll-wrap with-card-head">
            <Table<AgentGroupRow>
              className="data-table"
              rowKey="group_id"
              dataSource={groups}
              columns={columns}
              loading={isLoading}
              pagination={false}
              size="small"
              scroll={{ x: 'max-content', y: 'var(--table-scroll-y)' }}
            />
          </div>
        </Card>
      </div>

      <Modal
        title={editing ? '编辑组' : '新建组'}
        open={modalOpen}
        onOk={handleOk}
        onCancel={() => setModalOpen(false)}
        confirmLoading={createMutation.isPending || updateMutation.isPending}
      >
        <Form form={form} layout="vertical">
          <Form.Item name="group_name" label="组名" rules={[{ required: true, message: '请输入组名' }]}>
            <Input placeholder="请输入组名" />
          </Form.Item>
          <Form.Item name="agent_ids" label="采集机">
            <Select mode="multiple" placeholder="选择采集机" options={agentOptions} allowClear />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={3} placeholder="可选描述" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
