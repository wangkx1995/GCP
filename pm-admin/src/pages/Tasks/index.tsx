import { useState } from 'react';
import { Card, Form, Input, InputNumber, Select, DatePicker, Button, message, Alert, Divider, Tag, Space } from 'antd';
import { SendOutlined } from '@ant-design/icons';
import dayjs from 'dayjs';
import { useDispatchTask } from '../../api/hooks';
import { useSnapshots } from '../../api/hooks';

interface FormValues {
  task_id: string;
  logical_task_key: string;
  strategy_id: string;
  config_snapshot_id: string;
  scan_start_time: string;
  collect_id: string;
  load_type: string;
  encoding: string;
  output_delimiter: string;
  timeout_seconds: number;
  callback_base_url: string;
}

export default function TasksPage() {
  const [form] = Form.useForm<FormValues>();
  const mutation = useDispatchTask();
  const { data: snapshots } = useSnapshots();
  const [result, setResult] = useState<{ task_id: string; accepted: boolean; agent_task_state: string; reason: string | null } | null>(null);

  const handleSubmit = async (values: FormValues) => {
    try {
      const res = await mutation.mutateAsync({
        ...values,
        scan_start_time: dayjs(values.scan_start_time).format('YYYY-MM-DD HH:mm:ss'),
      });
      setResult(res);
      if (res.accepted) {
        message.success('任务已分发至 Agent');
      } else {
        message.warning(`任务被拒绝: ${res.reason ?? '未知原因'}`);
      }
    } catch (err) {
      message.error((err as Error).message);
    }
  };

  const handleReset = () => {
    form.resetFields();
    setResult(null);
  };

  return (
    <div>
      <div className="page-header">
        <h2>任务分发</h2>
        <p>向在线 Agent 分发采集任务</p>
      </div>

      {result && (
        <Alert
          type={result.accepted ? 'success' : 'warning'}
          style={{ marginBottom: 20, borderRadius: 6 }}
          message={
            <div>
              <strong>{result.accepted ? '分发成功' : '分发被拒'}</strong>
              <Divider type="vertical" />
              <span className="mono" style={{ fontSize: 13 }}>{result.task_id}</span>
              <Divider type="vertical" />
              <Tag color={result.accepted ? 'green' : 'red'}>{result.agent_task_state}</Tag>
              {result.reason && <Tag color="orange">{result.reason}</Tag>}
            </div>
          }
          closable
          action={<Button size="small" onClick={handleReset}>继续分发</Button>}
        />
      )}

      <Card className="content-card" style={{ maxWidth: 720 }}>
        <Form
          form={form}
          layout="vertical"
          onFinish={handleSubmit}
          initialValues={{
            load_type: 'clickhouse',
            encoding: 'UTF-8',
            output_delimiter: '|',
            timeout_seconds: 1800,
            callback_base_url: 'http://127.0.0.1:18080/api',
          }}
          disabled={mutation.isPending}
        >
          <div style={{ fontSize: 13, fontWeight: 600, color: '#64748b', marginBottom: 16, letterSpacing: '0.03em', textTransform: 'uppercase' }}>
            任务标识
          </div>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="task_id" label="任务 ID" rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="例: task_20260703_001" />
            </Form.Item>
            <Form.Item name="collect_id" label="采集标识" rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="例: collect_001" />
            </Form.Item>
          </Space>

          <Form.Item name="logical_task_key" label="去重键" rules={[{ required: true }]}>
            <Input placeholder="例: strategy_1:2026-06-17 15:15:00:v_20260703_120000" />
          </Form.Item>

          <div style={{ fontSize: 13, fontWeight: 600, color: '#64748b', margin: '20px 0 16px', letterSpacing: '0.03em', textTransform: 'uppercase' }}>
            采集参数
          </div>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="strategy_id" label="策略 ID" rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="例: strategy_1" />
            </Form.Item>
            <Form.Item name="config_snapshot_id" label="配置版本" rules={[{ required: true }]} style={{ flex: 1 }}>
              <Select
                placeholder="选择配置快照"
                showSearch
                optionFilterProp="label"
                options={snapshots?.map(s => ({
                  value: s.config_snapshot_id,
                  label: `${s.config_snapshot_id}${s.is_active ? ' (当前)' : ''}`,
                })) ?? []}
                dropdownRender={(menu) => (
                  <>
                    {menu}
                    {(!snapshots || snapshots.length === 0) && (
                      <div style={{ padding: 8, color: '#94a3b8', fontSize: 12, textAlign: 'center' }}>暂无快照</div>
                    )}
                  </>
                )}
              />
            </Form.Item>
          </Space>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="scan_start_time" label="数据时间" rules={[{ required: true }]} style={{ flex: 1 }}>
              <DatePicker
                showTime={{ format: 'HH:mm:ss' }}
                format="YYYY-MM-DD HH:mm:ss"
                style={{ width: '100%' }}
              />
            </Form.Item>
          </Space>

          <div style={{ fontSize: 13, fontWeight: 600, color: '#64748b', margin: '20px 0 16px', letterSpacing: '0.03em', textTransform: 'uppercase' }}>
            执行配置
          </div>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="load_type" label="入库类型" rules={[{ required: true }]} style={{ width: 160 }}>
              <Select options={[
                { value: 'clickhouse', label: 'ClickHouse' },
                { value: 'postgresql', label: 'PostgreSQL' },
              ]} />
            </Form.Item>
            <Form.Item name="encoding" label="编码" rules={[{ required: true }]} style={{ width: 140 }}>
              <Select options={[
                { value: 'UTF-8', label: 'UTF-8' },
                { value: 'GBK', label: 'GBK' },
                { value: 'GB2312', label: 'GB2312' },
                { value: 'ISO-8859-1', label: 'ISO-8859-1' },
              ]} />
            </Form.Item>
            <Form.Item name="output_delimiter" label="分隔符" rules={[{ required: true }]} style={{ width: 100 }}>
              <Select options={[
                { value: '|', label: '竖线 |' },
                { value: ',', label: '逗号 ,' },
                { value: '\t', label: '制表符 \\t' },
              ]} />
            </Form.Item>
            <Form.Item name="timeout_seconds" label="超时(秒)" rules={[{ required: true }]} style={{ width: 120 }}>
              <InputNumber min={60} max={86400} style={{ width: '100%' }} />
            </Form.Item>
          </Space>

          <Form.Item name="callback_base_url" label="回调地址">
            <Input placeholder="http://127.0.0.1:18080/api" />
          </Form.Item>

          <Form.Item style={{ marginTop: 24 }}>
            <Button type="primary" htmlType="submit" icon={<SendOutlined />} loading={mutation.isPending} size="large">
              分发任务
            </Button>
          </Form.Item>
        </Form>
      </Card>
    </div>
  );
}
