import { Table, Button, Space, Tag, message, Modal, Card } from 'antd';
import { UploadOutlined, DownloadOutlined, CheckCircleOutlined } from '@ant-design/icons';
import { useSnapshots, useActivateSnapshot } from '../../api/hooks';
import { downloadSnapshot } from '../../api/config-snapshots';
import { useState } from 'react';
import UploadModal from './UploadModal';
import type { ConfigSnapshotMeta } from '../../types/api';

export default function ConfigSnapshotsPage() {
  const { data: snapshots, isLoading } = useSnapshots();
  const activateMutation = useActivateSnapshot();
  const [uploadOpen, setUploadOpen] = useState(false);

  const handleActivate = (id: string) => {
    Modal.confirm({
      title: '确认激活',
      content: `确定激活采集适配器 ${id}？\n激活后所有在线采集机将自动下载新配置。`,
      onOk: async () => {
        try {
          await activateMutation.mutateAsync(id);
          message.success('激活成功，已通知在线采集机');
        } catch (e) {
          message.error(`激活失败: ${e instanceof Error ? e.message : '未知错误'}`);
        }
      },
    });
  };

  const columns = [
    {
      title: '文件名',
      dataIndex: 'name',
      key: 'name',
      render: (v: string | null) => v ?? <span style={{ color: '#94a3b8' }}>-</span>,
    },
    {
      title: '快照 ID',
      dataIndex: 'config_snapshot_id',
      key: 'id',
      ellipsis: true,
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '状态',
      key: 'active',
      align: 'center' as const,
      render: (_: unknown, record: ConfigSnapshotMeta) =>
        record.is_active ? (
          <Tag color="green" style={{ borderRadius: 4 }}>
            <span className="status-dot" style={{ background: '#10b981' }} />
            当前
          </Tag>
        ) : null,
    },
    {
      title: '创建时间',
      dataIndex: 'created_at',
      key: 'created',
    },
    {
      title: '激活时间',
      dataIndex: 'activated_at',
      key: 'activated',
      render: (v: string | null) => v ?? <span style={{ color: '#94a3b8' }}>-</span>,
    },
    {
      title: '操作',
      key: 'actions',
      render: (_: unknown, record: ConfigSnapshotMeta) => (
        <Space>
          <Button
            size="small"
            icon={<DownloadOutlined />}
            onClick={() => downloadSnapshot(record.config_snapshot_id)}
          >
            下载
          </Button>
          {!record.is_active && (
            <Button
              size="small"
              type="primary"
              icon={<CheckCircleOutlined />}
              loading={activateMutation.isPending}
              onClick={() => handleActivate(record.config_snapshot_id)}
            >
              激活
            </Button>
          )}
        </Space>
      ),
    },
  ];

  return (
    <div className="page-container">
      <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <h2>采集适配器管理</h2>
          <p>管理配置文件版本，上传 zip 包并激活生效</p>
        </div>
        <Button type="primary" icon={<UploadOutlined />} onClick={() => setUploadOpen(true)} size="large">
          上传配置
        </Button>
      </div>
      <div className="page-body">
      <Card className="content-card" styles={{ body: { padding: 0 } }}>
        <Table
          className="data-table"
          dataSource={snapshots}
          columns={columns}
          loading={isLoading}
          rowKey="config_snapshot_id"
          pagination={false}
        />
      </Card>
      </div>
      <UploadModal open={uploadOpen} onClose={() => setUploadOpen(false)} />
    </div>
  );
}
