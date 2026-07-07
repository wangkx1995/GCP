import { useEffect, useCallback, useMemo } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { Card, Form, Input, InputNumber, Select, Button, message } from 'antd';
import { SaveOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import { useDataCollectorUnits, useCreateStrategies, useStrategy, useUpdateStrategy, useAgents } from '../../api/hooks';
import type { CollectionStrategyCreateRequest, CollectionStrategyUpdateRequest } from '../../types/api';

function isValidCron(expr: string): boolean {
  const parts = expr.trim().split(/\s+/);
  if (parts.length !== 5) return false;
  const ranges = [
    { min: 0, max: 59 },
    { min: 0, max: 23 },
    { min: 1, max: 31 },
    { min: 1, max: 12 },
    { min: 0, max: 7 },
  ];
  return parts.every((part, i) => {
    if (part === '*') return true;
    return part.split(',').every(seg => {
      let [start, end] = seg.split('/');
      if (end !== undefined && !/^\d+$/.test(end)) return false;
      let [from, to] = start.split('-');
      if (from === '*' && to === undefined) return true;
      if (to !== undefined) {
        const a = Number(from), b = Number(to);
        if (isNaN(a) || isNaN(b) || a < ranges[i].min || b > ranges[i].max || a > b) return false;
      } else {
        const a = Number(from);
        if (isNaN(a) || a < ranges[i].min || a > ranges[i].max) return false;
      }
      return true;
    });
  });
}

export default function PeriodicStrategyPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const isNew = location.pathname.endsWith('/periodic');
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

  const agentNameMap = useMemo(() => new Map(agents?.map(a => [String(a.agent_id), a.agent_name]) ?? []), [agents]);
  const availableTables = selectedUnit?.table_names ?? [];

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

  useEffect(() => {
    if (editData) {
      form.setFieldsValue({
        collector_id: editData.collector_id,
        collector_name: editData.collector_name,
        collector_interval: editData.collect_interval,
        data_interval_seconds: editData.data_interval,
        table_names: editData.table_name ? [editData.table_name] : [],
        agent_ids: editData.agent_ids,
        cron_expression: editData.cron_expression,
        status: editData.status,
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
          cron_expression: values.cron_expression,
          collect_interval: values.collector_interval,
          data_interval: values.data_interval_seconds,
          agent_ids: JSON.stringify(values.agent_ids || []),
          strategy_type: 'periodic',
        };
        await createMutation.mutateAsync(data);
        message.success('创建成功');
      } else if (editId) {
        const data: CollectionStrategyUpdateRequest = {
          cron_expression: values.cron_expression,
          collect_interval: values.collector_interval,
          data_interval: values.data_interval_seconds,
          agent_ids: JSON.stringify(values.agent_ids || []),
          status: values.status,
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
          <h2 style={{ display: 'inline' }}>{isNew ? '新建周期性采集策略' : '编辑周期性采集策略'}</h2>
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
            <Form.Item name="cron_expression" label="采集时间(Crontab)" rules={[{ required: true, validator: (_, value) => !value || isValidCron(value) ? Promise.resolve() : Promise.reject(new Error('Crontab 格式无效，需 5 段 (分 时 日 月 周)')) }]}>
              <Input placeholder="0 0 * * *" />
            </Form.Item>
            <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择采集机" options={(units ?? []).find(u => u.id === watchedCollectorId)?.agent_ids.map(a => ({ label: agentNameMap.get(String(a)) ?? String(a), value: a })) ?? []} />
            </Form.Item>
            {!isNew && (
              <Form.Item name="status" label="状态">
                <Select options={[{ label: '可用', value: '可用' }, { label: '挂起', value: '挂起' }]} />
              </Form.Item>
            )}
          </Form>
        </Card>
      </div>
    </div>
  );
}
