import { useState } from 'react';
import { Card, Form, Input, InputNumber, Switch, Select, Button, message, Alert, Space, Divider, Tag } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { useRegisterAgent } from '../../api/hooks';

interface FormValues {
  agent_name: string;
  host: string;
  port: number;
  version: string;
  can_collect: boolean;
  can_parse: boolean;
  can_load: boolean;
  supported_protocols: string[];
}

export default function AgentsPage() {
  const [form] = Form.useForm<FormValues>();
  const mutation = useRegisterAgent();
  const [result, setResult] = useState<{ agent_id: string; heartbeat_interval: number; task_report_interval: number } | null>(null);

  const handleSubmit = async (values: FormValues) => {
    try {
      const res = await mutation.mutateAsync({
        agent_id: null,
        agent_name: values.agent_name,
        host: values.host,
        port: values.port,
        version: values.version,
        capabilities: {
          can_collect: values.can_collect,
          can_parse: values.can_parse,
          can_load: values.can_load,
          supported_protocols: values.supported_protocols,
        },
      });
      setResult({
        agent_id: res.agent_id,
        heartbeat_interval: res.heartbeat_interval_seconds,
        task_report_interval: res.task_report_interval_seconds,
      });
      message.success('Agent 注册成功');
    } catch {
      message.error('注册失败');
    }
  };

  const handleReset = () => {
    form.resetFields();
    setResult(null);
  };

  return (
    <div>
      <div className="page-header">
        <h2>Agent 管理</h2>
        <p>手动注册采集 Agent 到 Core 服务</p>
      </div>

      {result && (
        <Alert
          type="success"
          style={{ marginBottom: 20, borderRadius: 6 }}
          message={
            <div>
              <strong>注册成功</strong>
              <Divider type="vertical" />
              <span className="mono" style={{ fontSize: 13 }}>{result.agent_id}</span>
              <Divider type="vertical" />
              <Tag color="blue">心跳间隔: {result.heartbeat_interval}s</Tag>
              <Tag color="blue">上报间隔: {result.task_report_interval}s</Tag>
            </div>
          }
          closable
          action={<Button size="small" onClick={handleReset}>继续注册</Button>}
        />
      )}

      <Card className="content-card" style={{ maxWidth: 680 }}>
        <Form
          form={form}
          layout="vertical"
          onFinish={handleSubmit}
          initialValues={{
            port: 18081,
            version: '1.0.0',
            can_collect: true,
            can_parse: true,
            can_load: false,
            supported_protocols: ['ftp'],
          }}
          disabled={mutation.isPending}
        >
          <div style={{ fontSize: 13, fontWeight: 600, color: '#64748b', marginBottom: 16, letterSpacing: '0.03em', textTransform: 'uppercase' }}>
            基本信息
          </div>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="agent_name" label="名称" rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="例: agent-1" />
            </Form.Item>
            <Form.Item name="version" label="版本" rules={[{ required: true }]} style={{ width: 120 }}>
              <Input placeholder="1.0.0" />
            </Form.Item>
          </Space>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="host" label="主机地址" rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="例: 192.168.1.100" />
            </Form.Item>
            <Form.Item name="port" label="端口" rules={[{ required: true }]} style={{ width: 120 }}>
              <InputNumber min={1} max={65535} style={{ width: '100%' }} />
            </Form.Item>
          </Space>

          <div style={{ fontSize: 13, fontWeight: 600, color: '#64748b', margin: '20px 0 16px', letterSpacing: '0.03em', textTransform: 'uppercase' }}>
            能力配置
          </div>

          <Space size="middle" style={{ width: '100%' }} align="start">
            <Form.Item name="can_collect" label="采集" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="can_parse" label="解析" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="can_load" label="入库" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>

          <Form.Item name="supported_protocols" label="支持协议">
            <Select
              mode="multiple"
              placeholder="选择支持的协议"
              options={[
                { value: 'ftp', label: 'FTP' },
                { value: 'sftp', label: 'SFTP' },
                { value: 'local', label: '本地' },
              ]}
              style={{ maxWidth: 400 }}
            />
          </Form.Item>

          <Form.Item style={{ marginTop: 24 }}>
            <Button type="primary" htmlType="submit" icon={<PlusOutlined />} loading={mutation.isPending} size="large">
              注册 Agent
            </Button>
          </Form.Item>
        </Form>
      </Card>
    </div>
  );
}
