import { useState } from 'react';
import { Card, Select, DatePicker, Radio, Space, Alert, Spin } from 'antd';
import dayjs from 'dayjs';
import GridTable from './GridTable';
import { useGrid } from '../../api/hooks';

const STRATEGIES = ['strategy_1', 'strategy_2'];

export default function ResultsPage() {
  const [strategyId, setStrategyId] = useState<string>(STRATEGIES[0]);
  const [day, setDay] = useState(dayjs().format('YYYY-MM-DD'));
  const [interval, setInterval] = useState(15);

  const { data: grid, isLoading, isError } = useGrid({
    strategy_id: strategyId,
    day,
    interval_minutes: interval,
  });

  return (
    <div>
      <div className="page-header">
        <h2>结果网格</h2>
        <p>按策略和日期查看各表采集状态</p>
      </div>
      <Card className="content-card" style={{ marginBottom: 20 }}>
        <Space wrap size="middle">
          <div>
            <div style={{ fontSize: 12, color: '#64748b', marginBottom: 4, fontWeight: 500 }}>策略</div>
            <Select
              value={strategyId}
              onChange={setStrategyId}
              options={STRATEGIES.map(s => ({ value: s, label: s }))}
              style={{ width: 200 }}
              placeholder="选择策略"
            />
          </div>
          <div>
            <div style={{ fontSize: 12, color: '#64748b', marginBottom: 4, fontWeight: 500 }}>日期</div>
            <DatePicker
              value={dayjs(day)}
              onChange={d => d && setDay(d.format('YYYY-MM-DD'))}
              allowClear={false}
            />
          </div>
          <div>
            <div style={{ fontSize: 12, color: '#64748b', marginBottom: 4, fontWeight: 500 }}>时间间隔</div>
            <Radio.Group
              value={interval}
              onChange={e => setInterval(e.target.value)}
              optionType="button"
              options={[
                { value: 15, label: '15min' },
                { value: 30, label: '30min' },
                { value: 60, label: '60min' },
              ]}
            />
          </div>
        </Space>
      </Card>

      {isError && (
        <Alert
          type="error"
          message="加载失败"
          description="无法获取网格数据，请检查策略和日期是否正确"
          style={{ borderRadius: 6, marginBottom: 16 }}
        />
      )}
      {grid && <GridTable grid={grid} loading={isLoading} />}
      {!grid && !isError && isLoading && (
        <div className="spin-container"><Spin size="large" /></div>
      )}
    </div>
  );
}
