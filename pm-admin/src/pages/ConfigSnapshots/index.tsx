import { Table, Button, Space, Tag, message, Modal } from 'antd';
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
      content: `确定激活配置快照 ${id}？`,
      onOk: async () => {
        try {
          await activateMutation.mutateAsync(id);
          message.success('激活成功');
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
    },
    {
      title: '文件数',
      dataIndex: 'file_count',
      key: 'files',
      width: 80,
    },
    {
      title: 'Content Hash',
      dataIndex: 'content_hash',
      key: 'hash',
      ellipsis: true,
      width: 200,
    },
    {
      title: '状态',
      key: 'active',
      width: 80,
      render: (_: unknown, record: ConfigSnapshotMeta) =>
        record.is_active ? <Tag color="green">当前</Tag> : null,
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
      render: (v: string | null) => v ?? '-',
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
      <div style={{ marginBottom: 16, display: 'flex', justifyContent: 'space-between' }}>
        <h2 style={{ margin: 0 }}>配置快照</h2>
        <Button type="primary" icon={<UploadOutlined />} onClick={() => setUploadOpen(true)}>
          上传配置
        </Button>
      </div>
      <Table
        dataSource={snapshots}
        columns={columns}
        loading={isLoading}
        rowKey="config_snapshot_id"
        pagination={false}
      />
      <UploadModal open={uploadOpen} onClose={() => setUploadOpen(false)} />
    </div>
  );
}
