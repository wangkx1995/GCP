import { useState } from 'react';
import { Card, Select, Space, Typography, Spin } from 'antd';
import {
  LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer,
} from 'recharts';
import { useAgents, useAgentStatusHistory } from '../../api/hooks';

const { Text } = Typography;

export default function AgentHistoryPage() {
  const { data: agents } = useAgents();
  const [selectedId, setSelectedId] = useState<number | undefined>(undefined);
  const { data: history, isLoading } = useAgentStatusHistory(selectedId!, 200);

  const options = (agents || []).map(a => ({
    value: a.agent_id,
    label: `${a.agent_name} (${a.host}:${a.port})`,
  }));

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>状态历史</h2>
        <p>采集机负载趋势分析</p>
      </div>

      <div className="page-body">
        <Card className="content-card">
          <Space direction="vertical" style={{ width: '100%' }} size="large">
            <Select
              showSearch
              placeholder="选择采集机"
              options={options}
              value={selectedId}
              onChange={setSelectedId}
              style={{ width: 300 }}
              filterOption={(input, option) =>
                (option?.label as string)?.toLowerCase().includes(input.toLowerCase())
              }
            />

            {!selectedId && <Text type="secondary">请先选择一个采集机</Text>}
            {selectedId && isLoading && <Spin />}
            {selectedId && history && history.length > 0 && (
              <ResponsiveContainer width="100%" height={400}>
                <LineChart data={history}>
                  <CartesianGrid strokeDasharray="3 3" />
                  <XAxis
                    dataKey="heartbeat_time"
                    tick={{ fontSize: 11 }}
                    angle={-45}
                    textAnchor="end"
                    height={80}
                  />
                  <YAxis domain={[0, 100]} tickFormatter={v => `${v}%`} />
                  <Tooltip formatter={(v) => (v != null ? `${Number(v).toFixed(1)}%` : '—')} />
                  <Legend />
                  <Line type="monotone" dataKey="cpu_load" stroke="#3B82F6" name="CPU 负载" dot={false} />
                  <Line type="monotone" dataKey="memory_load" stroke="#22C55E" name="内存负载" dot={false} />
                  <Line type="monotone" dataKey="disk_load" stroke="#F59E0B" name="磁盘负载" dot={false} />
                </LineChart>
              </ResponsiveContainer>
            )}
            {selectedId && history && history.length === 0 && (
              <Text type="secondary">暂无历史数据</Text>
            )}
          </Space>
        </Card>
      </div>
    </div>
  );
}
