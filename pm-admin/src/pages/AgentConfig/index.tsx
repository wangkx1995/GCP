import { useState, useEffect, useCallback } from 'react';
import {
  Table, Card, Button, Form, Input, InputNumber, Select, message, Popconfirm,
  Space, Row, Col,
} from 'antd';
import { PlusOutlined, DeleteOutlined, SaveOutlined } from '@ant-design/icons';
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

export default function AgentConfigPage() {
  const { data: units, isLoading } = useDataCollectorUnits();
  const { data: agents } = useAgents();
  const nextIdMutation = useNextUnitId();
  const saveMutation = useSaveDataCollectorUnit();
  const deleteMutation = useDeleteDataCollectorUnit();

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [editing, setEditing] = useState(false);
  const [form] = Form.useForm();

  const selectedUnit = units?.find(u => u.id === selectedId);

  const [configSearch, setConfigSearch] = useState<string>('');
  const { data: configNamesData } = useConfigNames(configSearch);
  const configNames = configNamesData?.config_names ?? [];

  const watchedConfigName = Form.useWatch('config_name', form);
  const { data: tablesData } = useTablesForConfig(watchedConfigName);
  const availableTables = tablesData?.tables ?? [];

  useEffect(() => {
    if (selectedUnit && !editing) {
      const tableNames: string[] = tryParseJson(selectedUnit.table_names, []);
      const agentIdList: string[] = tryParseJson(selectedUnit.agent_ids, []);
      form.setFieldsValue({ ...selectedUnit, table_names: tableNames, agent_ids: agentIdList });
    } else if (!selectedUnit) {
      form.resetFields();
    }
  }, [selectedUnit, editing, form]);

  function tryParseJson(val: string, fallback: string[]) {
    try { return JSON.parse(val); } catch { return fallback; }
  }

  const handleNew = useCallback(async () => {
    const result = await nextIdMutation.mutateAsync();
    const newId = result.id;
    form.resetFields();
    form.setFieldsValue({ id: newId, collector_interval: 900, data_interval_seconds: 900 });
    setSelectedId(newId);
    setEditing(true);
  }, [nextIdMutation, form]);

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
      setEditing(false);
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [form, saveMutation]);

  const handleDelete = useCallback(async (id: number) => {
    try {
      await deleteMutation.mutateAsync(id);
      message.success('删除成功');
      if (selectedId === id) {
        setSelectedId(null);
      }
    } catch (e: unknown) {
      if (e instanceof Error) message.error(e.message);
    }
  }, [deleteMutation, selectedId]);

  const columns = [
    { title: 'ID', dataIndex: 'id', key: 'id', width: 60 },
    { title: '单元名称', dataIndex: 'unit_name', key: 'unit_name' },
    { title: '适配器名称', dataIndex: 'config_name', key: 'config_name' },
    { title: '适配器版本', dataIndex: 'config_version', key: 'config_version' },
    { title: '采集表', dataIndex: 'table_names', key: 'table_names', render: (v: string) => {
      try { return JSON.parse(v).join(', '); } catch { return v; }
    }},
    { title: '采集机', dataIndex: 'agent_ids', key: 'agent_ids', render: (v: string) => {
      try { return JSON.parse(v).join(', '); } catch { return v; }
    }},
    { title: '数据周期(秒)', dataIndex: 'data_interval_seconds', key: 'data_interval_seconds' },
    { title: '采集周期(秒)', dataIndex: 'collector_interval', key: 'collector_interval' },
    { title: '数据源', dataIndex: 'source_type', key: 'source_type' },
    { title: '主机', dataIndex: 'host', key: 'host' },
    { title: '端口', dataIndex: 'port', key: 'port' },
    { title: '用户名', dataIndex: 'username', key: 'username' },
    { title: '远程路径', dataIndex: 'remote_pattern', key: 'remote_pattern' },
    { title: '编码', dataIndex: 'file_encoding', key: 'file_encoding' },
    {
      title: '操作', key: 'action', width: 80,
      render: (_: unknown, record: DataCollectorUnit) => (
        <Popconfirm title="确认删除?" onConfirm={() => handleDelete(record.id)}>
          <Button danger size="small" icon={<DeleteOutlined />} loading={deleteMutation.isPending} />
        </Popconfirm>
      ),
    },
  ];

  return (
    <div>
      <div className="page-header">
        <h2>采集单元配置</h2>
        <p>管理采集单元，绑定适配器、采集机、数据源和调度配置</p>
      </div>

      <Row gutter={16}>
        <Col span={24} lg={10}>
          <Card
            title="采集单元列表"
            className="content-card"
            styles={{ body: { padding: 0 } }}
            extra={
              <Button type="primary" icon={<PlusOutlined />} onClick={handleNew} loading={nextIdMutation.isPending}>
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
              scroll={{ x: 'max-content' }}
              size="small"
              onRow={(record) => ({
                onClick: () => { setSelectedId(record.id); setEditing(false); },
                style: { cursor: 'pointer', background: selectedId === record.id ? '#E6F4FF' : undefined },
              })}
            />
          </Card>
        </Col>
        <Col span={24} lg={14}>
          <Card
            title={selectedId ? `编辑采集单元 #${selectedId}` : '选择或新建采集单元'}
            className="content-card"
          >
            <Form
              form={form}
              layout="vertical"
              disabled={!editing && !!selectedUnit}
              initialValues={{ collector_interval: 900, data_interval_seconds: 900 }}
            >
              <Form.Item name="id" hidden><InputNumber /></Form.Item>
              <Row gutter={16}>
                <Col span={12}>
                  <Form.Item name="unit_name" label="单元名称" rules={[{ required: true }]}>
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item name="config_name" label="适配器名称" rules={[{ required: true }]}>
                    <Select
                      showSearch
                      onSearch={setConfigSearch}
                      filterOption={false}
                      options={configNames.map(n => ({ label: `${n.name} (${n.version})`, value: n.name }))}
                    />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={12}>
                  <Form.Item name="table_names" label="采集表" rules={[{ required: true }]}>
                    <Select
                      mode="multiple"
                      options={availableTables.map(t => ({ label: t, value: t }))}
                    />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
                    <Select
                      mode="multiple"
                      options={(agents ?? []).map(a => ({ label: `${a.agent_name} (${a.agent_id})`, value: a.agent_id }))}
                    />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="data_interval_seconds" label="数据周期(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="collector_interval" label="采集周期(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="task_timeout_seconds" label="任务超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={60} />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="source_type" label="数据源类型">
                    <Select options={[
                      { label: 'SFTP', value: 'sftp' },
                      { label: 'FTP', value: 'ftp' },
                    ]} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="file_encoding" label="文件编码">
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="remote_pattern" label="远程文件路径">
                    <Input />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="host" label="主机地址">
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={4}>
                  <Form.Item name="port" label="端口">
                    <InputNumber style={{ width: '100%' }} min={1} max={65535} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="username" label="用户名">
                    <Input />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="password" label="密码">
                    <Input.Password />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={6}>
                  <Form.Item name="connect_retry" label="连接重试">
                    <InputNumber style={{ width: '100%' }} min={0} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="download_retry" label="下载重试">
                    <InputNumber style={{ width: '100%' }} min={0} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="download_parallel" label="并行下载数">
                    <InputNumber style={{ width: '100%' }} min={1} />
                  </Form.Item>
                </Col>
                <Col span={6}>
                  <Form.Item name="retry_interval_secs" label="重试间隔(秒)">
                    <InputNumber style={{ width: '100%' }} min={5} />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={16}>
                <Col span={8}>
                  <Form.Item name="connect_timeout_secs" label="连接超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={5} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="read_timeout_secs" label="读取超时(秒)">
                    <InputNumber style={{ width: '100%' }} min={10} />
                  </Form.Item>
                </Col>
                <Col span={8}>
                  <Form.Item name="cache_retention_days" label="缓存保留(天)">
                    <InputNumber style={{ width: '100%' }} min={1} />
                  </Form.Item>
                </Col>
              </Row>
              <Space>
                {!editing && selectedUnit ? (
                  <Button type="primary" onClick={() => setEditing(true)}>编辑</Button>
                ) : (
                  <Button
                    type="primary"
                    icon={<SaveOutlined />}
                    onClick={handleSave}
                    loading={saveMutation.isPending}
                  >
                    保存
                  </Button>
                )}
              </Space>
            </Form>
          </Card>
        </Col>
      </Row>
    </div>
  );
}
