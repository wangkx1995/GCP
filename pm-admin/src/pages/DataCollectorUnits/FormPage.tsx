import { useEffect, useCallback, useMemo, useRef, useState } from 'react';
import { useParams, useNavigate, useLocation } from 'react-router-dom';
import { Card, Form, Input, InputNumber, Select, Button, message, Divider, Collapse, Spin } from 'antd';
import { SaveOutlined, ArrowLeftOutlined } from '@ant-design/icons';
import {
  useDataCollectorUnits,
  useSaveDataCollectorUnit,
  useAgents,
  useAgentGroupList,
  useConfigNames,
  useTablesForConfig,
} from '../../api/hooks';
import type { DataCollectorUnitSaveRequest } from '../../types/api';

export default function DataCollectorUnitFormPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const isNew = location.pathname.endsWith('/create');
  const editId = isNew ? null : (id ?? null);

  const { data: units } = useDataCollectorUnits();
  const { data: agents } = useAgents();
  const { data: groups } = useAgentGroupList();
  const saveMutation = useSaveDataCollectorUnit();

  const [configSearch, setConfigSearch] = useState('');
  const { data: configNamesData } = useConfigNames(configSearch);
  const configNames = configNamesData?.config_names ?? [];

  const nameToVersion = useMemo(() => {
    const map: Record<string, string> = {};
    for (const item of configNames) map[item.name] = item.version;
    return map;
  }, [configNames]);

  const [form] = Form.useForm();
  const watchedConfigName = Form.useWatch('config_name', form);
  const watchedUnitName = Form.useWatch('unit_name', form);
  const { data: tablesData } = useTablesForConfig(watchedConfigName);
  const availableTables = tablesData?.tables ?? [];

  const agentOptions = useMemo(() => {
    const opts: { label: string; value: string }[] = [];
    for (const a of agents ?? []) {
      opts.push({ label: `${a.agent_alias} [采集机]`, value: String(a.agent_id) });
    }
    for (const g of groups ?? []) {
      opts.push({ label: `${g.group_name} [机组]`, value: `g:${g.group_id}` });
    }
    return opts;
  }, [agents, groups]);

  const selectedUnit = editId ? units?.find(u => u.id === editId) : null;

  const [idLoading, setIdLoading] = useState(false);

  const idInitRef = useRef(false);

  if (isNew && !idInitRef.current) {
    idInitRef.current = true;
    setIdLoading(true);
    fetch('/api/data-collector-units/next-id', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })
      .then(r => r.json())
      .then(data => {
        const id = data?.data?.id;
        if (id) form.setFieldsValue({ id });
      })
      .catch(() => message.error('获取ID失败'))
      .finally(() => setIdLoading(false));
  }

  useEffect(() => {
    if (selectedUnit) {
      form.setFieldsValue({ ...selectedUnit, agent_ids: selectedUnit.agent_ids?.map(String), password: undefined, db_password: undefined });
    }
  }, [selectedUnit, form]);

  const handleConfigNameChange = useCallback((value: string) => {
    if (nameToVersion[value]) {
      form.setFieldsValue({ config_version: nameToVersion[value] });
    }
  }, [form, nameToVersion]);

  const handleSave = useCallback(async () => {
    try {
      const values = await form.validateFields();
      const rawAgentIds = (values.agent_ids ?? []) as string[];
      const groupMap = new Map(groups?.map(g => [g.group_id, g.agent_ids.split(',').filter(Boolean)]) ?? []);
      const expandedIds: string[] = [];
      for (const v of rawAgentIds) {
        if (v.startsWith('g:')) {
          const gid = v.slice(2);
          const members = groupMap.get(gid);
          if (members) expandedIds.push(...members);
        } else {
          expandedIds.push(v);
        }
      }
      const data: DataCollectorUnitSaveRequest = {
        unit_name: values.unit_name,
        config_name: values.config_name,
        table_names: JSON.stringify(values.table_names || []),
        agent_ids: JSON.stringify([...new Set(expandedIds)]),
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
        load_type: values.load_type,
        output_delimiter: values.output_delimiter,
        db_host: values.db_host,
        db_port: values.db_port,
        db_user: values.db_user,
        db_password: values.db_password,
        db_database: values.db_database,
        db_table_name_case: values.db_table_name_case,
      };
      await saveMutation.mutateAsync({ id: values.id, data });
      message.success('保存成功');
      navigate('/data-collector-units');
    } catch (e: unknown) {
      if (e && typeof e === 'object' && 'errorFields' in e) {
        return;
      }
      if (e instanceof Error) message.error(e.message);
    }
  }, [form, saveMutation, navigate, groups]);

return (
    <div className="page-container">
      <div style={{
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        paddingBottom: 16,
        marginBottom: 16,
        position: 'sticky',
        top: 0,
        zIndex: 10,
        background: 'var(--color-bg-layout)',
      }}>
        <div>
          <Button type="text" icon={<ArrowLeftOutlined />} aria-label="返回" onClick={() => navigate('/data-collector-units')} style={{ marginRight: 8 }} />
          <h2 style={{ display: 'inline' }}>{idLoading ? '加载中...' : (isNew || !editId ? '新建采集单元' : `编辑 ${watchedUnitName || `采集单元 #${editId}`}`)}</h2>
        </div>
        <div>
          <Button onClick={() => navigate('/data-collector-units')} style={{ marginRight: 8 }}>取消</Button>
          <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={saveMutation.isPending}>保存</Button>
        </div>
      </div>

      <div style={{ flex: 1, overflowY: 'auto' }}>
        <Spin spinning={idLoading} tip="正在获取ID...">
          <Card className="content-card">
          <Form form={form} layout="vertical" initialValues={{
            collector_interval: 900,
            data_interval_seconds: 900,
            connect_retry: 3,
            download_retry: 3,
            download_parallel: 5,
            retry_interval_secs: 30,
            connect_timeout_secs: 30,
            read_timeout_secs: 300,
            cache_retention_days: 7,
            task_timeout_seconds: 3600,
            file_encoding: 'UTF-8',
            source_type: 'sftp',
            port: 22,
            load_type: 'clickhouse',
            output_delimiter: '|',
            db_table_name_case: 'lower',
          }}>
            <Form.Item name="id" hidden><InputNumber /></Form.Item>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>基本信息</Divider>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="unit_name" label="单元名称" rules={[{ required: true }]}><Input disabled={!isNew} placeholder="例如：机房A-北向指标" /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="config_name" label="适配器名称" rules={[{ required: true }]}>
                <Select showSearch onSearch={setConfigSearch} onChange={handleConfigNameChange} filterOption={false} placeholder="搜索并选择适配器" options={configNames.map(n => ({ label: n.name, value: n.name }))} />
              </Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="config_version" label="适配器版本"><Input disabled /></Form.Item></div>
            </div>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>采集配置</Divider>
            <Form.Item name="table_names" label="可采集表" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择要采集的表" options={availableTables.map(t => ({ label: t, value: t }))} />
            </Form.Item>
            <Form.Item name="agent_ids" label="采集机" rules={[{ required: true }]}>
              <Select mode="multiple" placeholder="选择采集机或机组" options={agentOptions} />
            </Form.Item>
            <Form.Item name="remote_pattern" label="远程文件路径" rules={[{ required: true }]}>
              <Input placeholder="/data/pm/{scan_start_time}_*.csv.gz" />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}><Form.Item name="data_interval_seconds" label="数据周期(秒)"><InputNumber style={{ width: '100%' }} min={60} placeholder="900" /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="collector_interval" label="采集周期(秒)"><InputNumber style={{ width: '100%' }} min={60} placeholder="900" /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="task_timeout_seconds" label="任务超时(秒)"><InputNumber style={{ width: '100%' }} min={60} placeholder="3600" /></Form.Item></div>
            </div>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>入库配置</Divider>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ flex: 1 }}>
                <Form.Item name="load_type" label="入库类型" rules={[{ required: true }]}>
                  <Select options={[
                    { value: 'clickhouse', label: 'ClickHouse' },
                    { value: 'postgresql', label: 'PostgreSQL' },
                  ]} />
                </Form.Item>
              </div>
              <div style={{ flex: 1 }}>
                <Form.Item name="output_delimiter" label="输出分隔符" rules={[{ required: true }]}>
                  <Select options={[
                    { value: '|', label: '竖线 |' },
                    { value: ',', label: '逗号 ,' },
                    { value: '\t', label: '制表符 \\t' },
                  ]} />
                </Form.Item>
              </div>
              <div style={{ flex: 1 }}>
                <Form.Item name="db_table_name_case" label="表名大小写" initialValue="lower">
                  <Select options={[
                    { value: 'lower', label: '小写' },
                    { value: 'upper', label: '大写' },
                  ]} />
                </Form.Item>
              </div>
            </div>
            <Form.Item name="db_host" label="数据库地址" rules={[{ required: true }]}>
              <Input placeholder="127.0.0.1" />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ width: 120 }}>
                <Form.Item name="db_port" label="端口" rules={[{ required: true }]}>
                  <InputNumber style={{ width: '100%' }} min={1} max={65535} placeholder="9000" />
                </Form.Item>
              </div>
              <div style={{ flex: 1 }}>
                <Form.Item name="db_user" label="数据库用户" rules={[{ required: true }]}>
                  <Input placeholder="default" />
                </Form.Item>
              </div>
              <div style={{ flex: 1 }}>
                <Form.Item name="db_password" label="数据库密码">
                  <Input.Password placeholder="可选" />
                </Form.Item>
              </div>
              <div style={{ flex: 1 }}>
                <Form.Item name="db_database" label="数据库名" rules={[{ required: true }]}>
                  <Input placeholder="default" />
                </Form.Item>
              </div>
            </div>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>数据源</Divider>
            <Form.Item name="source_type" label="类型">
              <Select options={[{ label: 'SFTP', value: 'sftp' }, { label: 'FTP', value: 'ftp' }]} />
            </Form.Item>
            <Form.Item name="host" label="主机地址" rules={[{ required: true }]}>
              <Input placeholder="192.168.1.100" />
            </Form.Item>
            <div style={{ display: 'flex', gap: 16 }}>
              <div style={{ width: 120 }}><Form.Item name="port" label="端口"><InputNumber style={{ width: '100%' }} min={1} max={65535} placeholder="22" /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="username" label="用户名" rules={[{ required: true }]}><Input placeholder="collector" /></Form.Item></div>
              <div style={{ flex: 1 }}><Form.Item name="password" label="密码" rules={isNew ? [{ required: true }] : []}><Input.Password placeholder={isNew ? '请输入密码' : '留空则保持原密码'} /></Form.Item></div>
            </div>
            <Form.Item name="file_encoding" label="文件编码">
              <Input placeholder="UTF-8" />
            </Form.Item>

            <Divider titlePlacement="left" style={{ fontSize: 14, fontWeight: 600 }}>高级设置</Divider>
            <Collapse ghost size="small" defaultActiveKey={[]}
              items={[{
                key: 'advanced', label: '展开高级设置',
                style: { background: 'var(--color-surface-hover)', borderRadius: 6 },
                children: (
                  <>
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
                  </>
                ),
              }]}
            />
          </Form>
        </Card>
        </Spin>
      </div>
    </div>
  );
}
