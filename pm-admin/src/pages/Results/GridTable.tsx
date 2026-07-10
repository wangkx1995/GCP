import { Table, Tooltip, Card } from 'antd';
import { GRID_COLORS } from '../../types/enums';
import type { DailyGrid, GridCell } from '../../types/api';
import { useMemo } from 'react';

interface Props {
  grid: DailyGrid;
  loading: boolean;
}

export default function GridTable({ grid, loading }: Props) {
  const columns = useMemo(() => [
    {
      title: '表名',
      dataIndex: 'table_name',
      key: 'table_name',
      fixed: 'left' as const,
      width: 130,
      render: (v: string) => <span style={{ fontWeight: 500, fontSize: 12 }}>{v}</span>,
    },
    ...grid.time_slots.map(slot => ({
      title: slot.slice(11, 16),
      key: slot,
      width: 64,
      render: (_: unknown, record: { table_name: string }) => {
        const row = grid.rows.find(r => r.table_name === record.table_name);
        const cell: GridCell | undefined = row?.cells.find(c => c.data_time === slot);
        if (!cell) {
          const gray = GRID_COLORS.gray.color;
          return <div className="grid-cell" style={{ background: gray, color: '#94a3b8' }}>-</div>;
        }

        const info = GRID_COLORS[cell.color];
        if (cell.color === 'none') {
          return <div className="grid-cell" style={{ background: 'transparent', color: '#e2e8f0' }}>-</div>;
        }
        return (
          <Tooltip
            title={
              <div style={{ fontSize: 12, lineHeight: 1.6 }}>
                <div>采集时间: {cell.data_time} ~ {cell.scan_end_time}</div>
                <div>行数: {cell.value?.toLocaleString() ?? '-'}</div>
                <div>预期行数: {cell.expected_rows_num?.toLocaleString() ?? '-'}</div>
                <div>完整率: {cell.completion_rate != null ? (cell.completion_rate * 100).toFixed(1) + '%' : '-'}</div>
                <div>状态: {info.label}</div>
              </div>
            }
          >
            <div
              className="grid-cell"
              style={{
                background: info.color,
                color: ['gray', 'yellow'].includes(cell.color) ? '#64748b' : '#fff',
              }}
            >
              {cell.value?.toLocaleString() ?? '-'}
            </div>
          </Tooltip>
        );
      },
    })),
  ], [grid]);

  const dataSource = useMemo(() =>
    grid.rows.map(r => ({ table_name: r.table_name })),
  [grid]);

  return (
    <Card className="content-card" styles={{ body: { padding: 0 } }}>
      <Table
        dataSource={dataSource}
        columns={columns}
        loading={loading}
        rowKey="table_name"
        scroll={{ x: 'max-content' }}
        pagination={false}
        bordered
        size="small"
        className="data-table"
      />
    </Card>
  );
}
