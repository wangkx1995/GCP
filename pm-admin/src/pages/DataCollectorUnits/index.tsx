import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Table, Card, Button, Form, Input, InputNumber, Select, message, Popconfirm,
  Modal, Divider, Collapse,
} from 'antd';
import { PlusOutlined, DeleteOutlined, EditOutlined, SaveOutlined } from '@ant-design/icons';
import {
  useDataCollectorUnits,
  useNextUnitId,
  useSaveDataCollectorUnit,
  useDeleteDataCollectorUnit,
  useAgents,
  useConfigNames,
  useTablesForConfig,
} from '../../api/hooks';
import type { DataCollectorUnit, DataCollectorUnitSaveRequest } from '../../types/api';

function tryParseJson(val: string, fallback: string[]) {
  try { return JSON.parse(val); } catch { return fallback; }
}

export default function DataCollectorUnitsPage() {
  const { data: units, isLoading } = useDataCollectorUnits();
  const { data: agents } = useAgents();
  const nextIdMutation = useNextUnitId();
  const saveMutation = useSaveDataCollectorUnit();
  const deleteMutation = useDeleteDataCollectorUnit();

  const [modalOpen, setModalOpen] = useState(false);
  const [editId, setEditId] = useState<number | null>(null);
  const [form] = Form.useForm();

  const [configSearch, setConfigSearch] = useState<string>('');
  const { data: configNamesData } = useConfigNames(configSearch);
  const configNames = configNamesData?.config_names ?? [];

  const nameToVersion = useMemo(() => {
    const map: Record<string, string> = {};
    for (const item of configNames) map[item.name] = item.version;
    return map;
  }, [configNames]);

  const watchedConfigName = Form.useWatch('config_name', form);
  const { data: tablesData } = useTablesForConfig(watchedConfigName);
  const availableTables = tablesData?.tables ?? [];

  const selectedUnit = editId ? units?.find(u => u.id === editId) : null;

  useEffect(() => {
    if (selectedUnit) {
      const tableNames: string[] = tryParseJson(selectedUnit.table_names, []);
      const agentIdList: string[] = tryParseJson(selectedUnit.agent_ids, []);
      form.setFieldsValue({ ...selectedUnit, table_names: tableNames, agent_ids: agentIdList });
    } else {
      form.resetFields();
    }
  }, [selectedUnit, form]);

  const handleConfigNameChange = useCallback((value: string) => {
    if (nameToVersion[value]) {
      form.setFieldsValue({ config_version: nameToVersion[value] });
    }
  }, [form, nameToVersion]);

  const openNew = useCallback(async () => {
    const result = await nextIdMutation.mutateAsync();
    form.resetFields();
    form.setFieldsValue({ id: result.id, collector_interval: 900, data_interval_seconds: 900 });
    setEditId(null);
    setModalOpen(true);
  }, [nextIdMutation, form]);

  const openEdit = useCallback((id: number) => {
    setEditId(id);
    setModalOpen(true);
  }, []);

  const closeModal = useCallback(() => {
    setModalOpen(false);
    setEditId(null);
  }, []);

  const handleSave = useCallback(async () => {
    try {
      const values = await form.validateFields();
      const id = values.id;
      const saveData: DataCollectorUnitSaveRequest = {
        unit_name: values.unit_name,
        config_name: values.config_name,
        table_names: JSON.stringify(values.table_names || []),
        agent_ids: JSON.stringify(values.agent_ids || []),
        data_interval_seconds: values.data_interval_seconds,
        collector_interval: values.collector_interval,
        task_timeout_seconds: values.task_timeout_seconds,
        source_type: values.source_type,
        file_encoding: values.file_encoding,
        remote_pattern: values.remote_pattern,
        host: values.host,
        port: values.port,
        username: values.username,
        password: values.password,
        connect_retry: values.connect_retry,
        download_retry: values.download_retry,
        download_parallel: values.download_parallel,
        retry_interval_secs: values.retry_interval_secs,
        connect_timeout_secs: values.connect_timeout_secs,
        read_timeout_secs: values.read_timeout_secs,
        cache_retention_days: values.cache_retention_days,
      };
      await saveMutation.mutateAsync({ id, data: saveData });
      message.success('保存成功');
      closeModal();
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [form, saveMutation, closeModal]);

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
          <Button type="link" size="small" icon={<EditOutlined />} onClick={() => openEdit(record.id)} />
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
        <h2>采集单元配置</h2>
        <p>管理采集单元，绑定适配器、采集机、数据源和调度配置</p>
      </div>

      <Card
        title="采集单元列表"
        className="content-card"
        styles={{ body: { padding: 0 } }}
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={openNew} loading={nextIdMutation.isPending}>
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
            onClick: () => openEdit(record.id),
            style: { cursor: 'pointer' },
          })}
        />
      </Card>

      <Modal
        title={editId ? `编辑采集单元 #${editId}` : '新建采集单元'}
        open={modalOpen}
        onCancel={closeModal}
        width={840}
        footer={null}
        destroyOnClose
        styles={{ body: { maxHeight: 'calc(100vh - 200px)', overflowY: 'auto', paddingTop: 8 } }}
      >
<Form
            form={form}
            layout="vertical"
            initialValues={{ collector_interval: 900, data_interval_seconds: 900 }}
          >
            <Form.Item name="id" hidden><InputNumber /></Form.Item>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>基本信息</Divider>
            <div style={{ padding: '0 8px' }}>
              <Form.Item name="unit_name" label="单元名称" rules={[{ required: true }]}>
                <Input placeholder="例如：机房A-北向指标" />
              </Form.Item>
              <Form.Item name="config_name" label="适配器名称" rules={[{ required: true }]}>
                <Select
                  showSearch
                  onSearch={setConfigSearch}
                  onChange={handleConfigNameChange}
                  filterOption={false}
                  placeholder="搜索并选择适配器"
                  options={configNames.map(n => ({ label: n.name, value: n.name }))}
                />
              </Form.Item>
              <Form.Item name="config_version" label="适配器版本">
                <Input disabled />
              </Form.Item>
            </div>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>采集配置</Divider>
            <div style={{ padding: '0 8px' }}>
              <Form.Item name="table_names" label="采集表" rules={[{ required: true }]}>
                <Select mode="multiple" placeholder="选择要采集的表" options={availableTables.map(t => ({ label: t, value: t }))} />
              </Form.Item>
              <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
                <Select mode="multiple" placeholder="选择采集机" options={(agents ?? []).map(a => ({ label: `${a.agent_name} (${a.agent_id})`, value: a.agent_id }))} />
              </Form.Item>
              <Form.Item name="remote_pattern" label="远程文件路径">
                <Input placeholder="/data/pm/{scan_start_time}_*.csv.gz" />
              </Form.Item>
              <div style={{ display: 'flex', gap: 16 }}>
                <div style={{ flex: 1 }}>
                  <Form.Item name="data_interval_seconds" label="数据周期(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} placeholder="900" />
                  </Form.Item>
                </div>
                <div style={{ flex: 1 }}>
                  <Form.Item name="collector_interval" label="采集周期(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} placeholder="900" />
                  </Form.Item>
                </div>
                <div style={{ flex: 1 }}>
                  <Form.Item name="task_timeout_seconds" label="任务超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} placeholder="3600" />
                  </Form.Item>
                </div>
              </div>
            </div>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>数据源</Divider>
            <div style={{ padding: '0 8px' }}>
              <Form.Item name="source_type" label="类型">
                <Select options={[{ label: 'SFTP', value: 'sftp' }, { label: 'FTP', value: 'ftp' }]} />
              </Form.Item>
              <Form.Item name="host" label="主机地址">
                <Input placeholder="192.168.1.100" />
              </Form.Item>
              <div style={{ display: 'flex', gap: 16 }}>
                <div style={{ width: 120 }}>
                  <Form.Item name="port" label="端口">
                    <InputNumber style={{ width: '100%' }} min={1} max={65535} placeholder="22" />
                  </Form.Item>
                </div>
                <div style={{ flex: 1 }}>
                  <Form.Item name="username" label="用户名">
                    <Input placeholder="collector" />
                  </Form.Item>
                </div>
                <div style={{ flex: 1 }}>
                  <Form.Item name="password" label="密码">
                    <Input.Password placeholder="留空则保持原密码" />
                  </Form.Item>
                </div>
              </div>
              <Form.Item name="file_encoding" label="文件编码">
                <Input placeholder="UTF-8" />
              </Form.Item>
            </div>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>高级设置</Divider>
            <Collapse
              ghost
              size="small"
              defaultActiveKey={[]}
              items={[{
                key: 'advanced',
                label: '展开高级设置',
                style: { background: '#FAFAFA', borderRadius: 6 },
                children: (
                  <div style={{ padding: '0 8px' }}>
                    <div style={{ display: 'flex', gap: 16 }}>
                      <div style={{ flex: 1 }}><Form.Item name="connect_retry" label="连接重试"><InputNumber style={{ width: '100%' }} min={0} placeholder="3" /></Form.Item></div>
                      <div style={{ flex: 1 }}><Form.Item name="download_retry" label="下载重试"><InputNumber style={{ width: '100%' }} min={0} placeholder="3" /></Form.Item></div>
                      <div style={{ flex: 1 }}><Form.Item name="download_parallel" label="并行下载数"><InputNumber style={{ width: '100%' }} min={1} placeholder="4" /></Form.Item></div>
                      <div style={{ flex: 1 }}><Form.Item name="retry_interval_secs" label="重试间隔(秒)"><InputNumber style={{ width: '100%' }} min={5} placeholder="30" /></Form.Item></div>
                    </div>
                    <div style={{ display: 'flex', gap: 16 }}>
                      <div style={{ flex: 1 }}><Form.Item name="connect_timeout_secs" label="连接超时(秒)"><InputNumber style={{ width: '100%' }} min={5} placeholder="30" /></Form.Item></div>
                      <div style={{ flex: 1 }}><Form.Item name="read_timeout_secs" label="读取超时(秒)"><InputNumber style={{ width: '100%' }} min={10} placeholder="300" /></Form.Item></div>
                      <div style={{ flex: 1 }}><Form.Item name="cache_retention_days" label="缓存保留(天)"><InputNumber style={{ width: '100%' }} min={1} placeholder="7" /></Form.Item></div>
                    </div>
                  </div>
                ),
              }]}
            />

          <div style={{ textAlign: 'right', marginTop: 24 }}>
            <Button onClick={closeModal} style={{ marginRight: 8 }}>取消</Button>
            <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={saveMutation.isPending}>保存</Button>
          </div>
        </Form>
      </Modal>
    </div>
  );
}