import { useState, useMemo } from 'react';
import { Card, Select, DatePicker, Space, Alert, Spin, Input } from 'antd';
import dayjs from 'dayjs';
import GridTable from './GridTable';
import { useGrid, useDataCollectorUnits } from '../../api/hooks';

export default function ResultsPage() {
  const { data: units } = useDataCollectorUnits();
  const unitOptions = useMemo(() =>
    (units ?? []).map(u => ({ value: u.unit_name, label: u.unit_name })),
    [units],
  );
  const [collectorName, setCollectorName] = useState<string>('');
  const [day, setDay] = useState(dayjs().format('YYYY-MM-DD'));
  const selectedUnit = useMemo(() => units?.find(u => u.unit_name === collectorName), [units, collectorName]);

  const { data: grid, isLoading, isError } = useGrid({
    collector_name: collectorName,
    day,
  });

  return (
    <div className="page-container">
      <div className="page-header">
        <h2>结果网格</h2>
        <p>按采集单元和日期查看各表数据完整性</p>
      </div>
      <Card className="content-card" style={{ marginBottom: 20 }}>
        <Space wrap size="middle">
          <div>
            <div style={{ fontSize: 12, color: '#64748b', marginBottom: 4, fontWeight: 500 }}>采集单元</div>
            <Select
              value={collectorName}
              onChange={setCollectorName}
              options={unitOptions}
              style={{ width: 240 }}
              placeholder="选择采集单元"
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
          {selectedUnit && (
            <div>
              <div style={{ fontSize: 12, color: '#64748b', marginBottom: 4, fontWeight: 500 }}>周期</div>
              <Input
                value={`${selectedUnit.collector_interval / 60} 分钟 (${selectedUnit.collector_interval} 秒)`}
                disabled
                style={{ width: 200 }}
              />
            </div>
          )}
        </Space>
      </Card>

      {isError && (
        <Alert
          type="error"
          message="加载失败"
          description="无法获取网格数据，请检查采集单元和日期是否正确"
          style={{ borderRadius: 6, marginBottom: 16 }}
        />
      )}
      {grid && !isLoading && <GridTable grid={grid} loading={false} />}
      {!grid && !isError && isLoading && (
        <div className="spin-container"><Spin size="large" /></div>
      )}
    </div>
  );
}
