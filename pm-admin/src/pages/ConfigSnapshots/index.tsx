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
      content: `确定激活配置快照 ${id}？\n激活后所有在线 Agent 将自动下载新配置。`,
      onOk: async () => {
        try {
          await activateMutation.mutateAsync(id);
          message.success('激活成功，已通知在线 Agent');
        } catch {
          message.error('激活失败');
        }
      },
    });
  };

  const columns = [
    {
      title: '快照 ID',
      dataIndex: 'config_snapshot_id',
      key: 'id',
      ellipsis: true,
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: '文件数',
      dataIndex: 'file_count',
      key: 'files',
      width: 80,
      align: 'center' as const,
    },
    {
      title: 'Content Hash',
      dataIndex: 'content_hash',
      key: 'hash',
      ellipsis: true,
      width: 220,
      render: (v: string) => <span className="mono" style={{ color: '#64748b', fontSize: 12 }}>{v}</span>,
    },
    {
      title: '版本标签',
      dataIndex: 'version_label',
      key: 'version',
      width: 120,
      render: (v: string | null) => v ?? <span style={{ color: '#94a3b8' }}>-</span>,
    },
    {
      title: '状态',
      key: 'active',
      width: 80,
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
      width: 180,
    },
    {
      title: '激活时间',
      dataIndex: 'activated_at',
      key: 'activated',
      width: 180,
      render: (v: string | null) => v ?? <span style={{ color: '#94a3b8' }}>-</span>,
    },
    {
      title: '操作',
      key: 'actions',
      width: 200,
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
    <div>
      <div className="page-header" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <h2>配置快照</h2>
          <p>管理配置文件版本，上传 zip 包并激活生效</p>
        </div>
        <Button type="primary" icon={<UploadOutlined />} onClick={() => setUploadOpen(true)} size="large">
          上传配置
        </Button>
      </div>
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
      <UploadModal open={uploadOpen} onClose={() => setUploadOpen(false)} />
    </div>
  );
}
