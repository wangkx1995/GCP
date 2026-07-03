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
      <Card style={{ marginBottom: 16 }}>
        <Space wrap>
          <Select
            value={strategyId}
            onChange={setStrategyId}
            options={STRATEGIES.map(s => ({ value: s, label: s }))}
            style={{ width: 200 }}
            placeholder="选择策略"
          />
          <DatePicker
            value={dayjs(day)}
            onChange={d => d && setDay(d.format('YYYY-MM-DD'))}
            allowClear={false}
          />
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
        </Space>
      </Card>

      {isError && <Alert type="error" message="加载失败" />}
      {grid && <GridTable grid={grid} loading={isLoading} />}
      {!grid && !isError && isLoading && <Spin />}
    </div>
  );
}
