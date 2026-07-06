import { useState, useMemo } from 'react';
import { Card, Select, DatePicker, Space, Alert, Spin } from 'antd';
import dayjs from 'dayjs';
import GridTable from './GridTable';
import { useGrid, useStrategies } from '../../api/hooks';

export default function ResultsPage() {
  const { data: strategies } = useStrategies();
  const strategyOptions = useMemo(() =>
    (strategies ?? []).map(s => ({ value: s.id.toString(), label: `${s.table_name} (ID: ${s.id})` })),
    [strategies],
  );
  const [strategyId, setStrategyId] = useState<string>('');
  const [day, setDay] = useState(dayjs().format('YYYY-MM-DD'));

  const { data: grid, isLoading, isError } = useGrid({
    strategy_id: strategyId,
    day,
  });

  return (
    <div className="page-container">
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
              options={strategyOptions}
              style={{ width: 240 }}
              placeholder="选择策略"
              showSearch
              filterOption={(input, option) =>
                (option?.label ?? '').toLowerCase().includes(input.toLowerCase())
              }
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
