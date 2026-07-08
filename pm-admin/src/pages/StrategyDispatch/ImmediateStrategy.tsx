import { useEffect, useCallback, useMemo } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { Card, Form, Input, InputNumber, Select, Button, message, DatePicker } from 'antd';
import { SaveOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import { useDataCollectorUnits, useCreateStrategies, useStrategy, useUpdateStrategy, useAgents } from '../../api/hooks';
import type { CollectionStrategyCreateRequest, CollectionStrategyUpdateRequest } from '../../types/api';
import dayjs from 'dayjs';

export default function ImmediateStrategyPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const isNew = location.pathname.endsWith('/immediate');
  const editId = isNew ? null : (id ?? null);

  const { data: units } = useDataCollectorUnits();
  const { data: agents } = useAgents();
  const { data: editData } = useStrategy(editId);
  const createMutation = useCreateStrategies();
  const updateMutation = useUpdateStrategy();

  const [form] = Form.useForm();
  const watchedCollectorId = Form.useWatch('collector_id', form);

  const selectedUnit = useMemo(() => {
    if (!watchedCollectorId || !units) return null;
    return units.find(u => u.id === watchedCollectorId) || null;
  }, [watchedCollectorId, units]);

  const agentOptions = useMemo(() => {
    const m = new Map<string, string>();
    (agents ?? []).forEach(a => m.set(String(a.agent_id), a.agent_alias || a.agent_name));
    return m;
  }, [agents]);
  const availableTables = selectedUnit?.table_names ?? [];

  // Auto-fill when unit selected
  useEffect(() => {
    if (selectedUnit) {
      form.setFieldsValue({
        collector_name: selectedUnit.unit_name,
        collector_interval: selectedUnit.collector_interval,
        data_interval_seconds: selectedUnit.data_interval_seconds,
        agent_ids: selectedUnit.agent_ids,
      });
    }
  }, [selectedUnit, form]);

  // Load edit data
  useEffect(() => {
    if (editData) {
      form.setFieldsValue({
        collector_id: editData.collector_id,
        collector_name: editData.collector_name,
        collector_interval: editData.collect_interval,
        data_interval_seconds: editData.data_interval,
        table_names: editData.table_name ? [editData.table_name] : [],
        agent_ids: editData.agent_ids,
        data_start_time: editData.data_start_time ? dayjs(editData.data_start_time) : undefined,
        data_end_time: editData.data_end_time ? dayjs(editData.data_end_time) : undefined,
        execute_time: editData.execute_time ? dayjs(editData.execute_time) : undefined,
      });
    }
  }, [editData, form]);

  const handleSave = useCallback(async () => {
    try {
      const values = await form.validateFields();
      if (isNew) {
        const data: CollectionStrategyCreateRequest = {
          collector_id: values.collector_id,
          collector_name: values.collector_name,
          table_names: values.table_names || [],
          collect_interval: values.collector_interval,
          data_interval: values.data_interval_seconds,
          data_start_time: values.data_start_time?.format('YYYY-MM-DD HH:mm:ss'),
          data_end_time: values.data_end_time?.format('YYYY-MM-DD HH:mm:ss'),
          execute_time: values.execute_time?.format('YYYY-MM-DD HH:mm:ss'),
          agent_ids: JSON.stringify(values.agent_ids || []),
          strategy_type: 'immediate',
        };
        await createMutation.mutateAsync(data);
        message.success('创建成功，任务已执行');
      } else if (editId) {
        const data: CollectionStrategyUpdateRequest = {
          collect_interval: values.collector_interval,
          data_interval: values.data_interval_seconds,
          data_start_time: values.data_start_time?.format('YYYY-MM-DD HH:mm:ss'),
          data_end_time: values.data_end_time?.format('YYYY-MM-DD HH:mm:ss'),
          execute_time: values.execute_time?.format('YYYY-MM-DD HH:mm:ss'),
          agent_ids: JSON.stringify(values.agent_ids || []),
        };
        await updateMutation.mutateAsync({ id: editId, data });
        message.success('更新成功');
      }
      navigate('/strategy-dispatch/info');
    } catch (e: unknown) {
      if (e && typeof e === 'object' && 'errorFields' in e) {
        return;
      }
      if (e instanceof Error) message.error(e.message);
    }
  }, [form, isNew, editId, createMutation, updateMutation, navigate]);

  return (
    <div className="page-container">
      <div style={{ flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingBottom: 16, marginBottom: 16, position: 'sticky', top: 0, zIndex: 10, background: 'var(--color-bg-layout)' }}>
        <div>
          <Button type="text" icon={<ArrowLeftOutlined />} aria-label="返回" onClick={() => navigate('/strategy-dispatch/info')} style={{ marginRight: 8 }} />
          <h2 style={{ display: 'inline' }}>{isNew ? '新建及时采集策略' : '编辑及时采集策略'}</h2>
        </div>
        <div>
          <Button onClick={() => navigate('/strategy-dispatch/info')} style={{ marginRight: 8 }}>取消</Button>
          <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={createMutation.isPending || updateMutation.isPending}>保存</Button>
        </div>
      </div>

      <div style={{ flex: 1, overflowY: 'auto' }}>
        <Card className="content-card">
          <Form form={form} layout="vertical">
            <Form.Item name="collector_id" label="采集单元" rules={[{ required: true }]}>
              <Select showSearch placeholder="搜索并选择采集单元" filterOption={(input, option) => (option?.label as string ?? '').includes(input)}
                options={(units ?? []).map(u => ({ label: u.unit_name, value: u.id }))} disabled={!isNew} />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="collector_name" label="采集单元名称"><Input disabled /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="collector_interval" label="采集周期(秒)"><InputNumber disabled style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="data_interval_seconds" label="数据周期(秒)"><InputNumber disabled style={{ width: '100%' }} /></Form.Item></div>
            </div>
            <Form.Item name="table_names" label="指标组(表名)" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择表名" options={availableTables.map(t => ({ label: t, value: t }))} disabled={!isNew} />
            </Form.Item>
            <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择采集机" options={(units ?? []).find(u => u.id === watchedCollectorId)?.agent_ids.map(a => ({ label: agentOptions.get(String(a)) ?? String(a), value: a })) ?? []} />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="data_start_time" label="数据开始时间"><DatePicker showTime style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="data_end_time" label="数据结束时间"><DatePicker showTime style={{ width: '100%' }} /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="execute_time" label="执行时间"><DatePicker showTime style={{ width: '100%' }} /></Form.Item></div>
            </div>
          </Form>
        </Card>
      </div>
    </div>
  );
}
