import { Table, Tooltip } from 'antd';
import { GRID_COLORS } from '../../types/enums';
import type { DailyGrid, GridCell } from '../../types/api';

interface Props {
  grid: DailyGrid;
  loading: boolean;
}

export default function GridTable({ grid, loading }: Props) {
  const columns = [
    {
      title: '表名',
      dataIndex: 'table_name',
      key: 'table_name',
      fixed: 'left' as const,
      width: 120,
    },
    ...grid.time_slots.map(slot => ({
      title: slot.slice(11, 16),
      key: slot,
      width: 72,
      render: (_: unknown, record: { table_name: string }) => {
        const row = grid.rows.find(r => r.table_name === record.table_name);
        const cell: GridCell | undefined = row?.cells.find(c => c.data_time === slot);
        if (!cell) return <div style={{ background: GRID_COLORS.gray.color, height: 24 }} />;

        const info = GRID_COLORS[cell.color];
        return (
          <Tooltip title={`${cell.data_time}\n行数: ${cell.value ?? '-'}\n状态: ${info.label}`}>
            <div
              style={{
                background: info.color,
                height: 24,
                borderRadius: 2,
                cursor: 'pointer',
                textAlign: 'center',
                lineHeight: '24px',
                color: cell.color === 'gray' ? '#999' : '#fff',
                fontSize: 11,
              }}
            >
              {cell.value ?? '-'}
            </div>
          </Tooltip>
        );
      },
    })),
  ];

  const dataSource = grid.rows.map(r => ({ table_name: r.table_name }));

  return (
    <Table
      dataSource={dataSource}
      columns={columns}
      loading={loading}
      rowKey="table_name"
      scroll={{ x: 'max-content' }}
      pagination={false}
      bordered
      size="small"
    />
  );
}
