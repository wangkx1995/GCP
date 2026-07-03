import { Modal, Upload, Alert, message } from 'antd';
import { InboxOutlined } from '@ant-design/icons';
import { useUploadSnapshot } from '../../api/hooks';
import type { UploadErrorResponse } from '../../types/api';

const { Dragger } = Upload;

interface Props {
  open: boolean;
  onClose: () => void;
}

export default function UploadModal({ open, onClose }: Props) {
  const mutation = useUploadSnapshot();

  const handleUpload = async (file: File) => {
    try {
      const result = await mutation.mutateAsync(file);
      if (!result.valid) {
        const err = result as UploadErrorResponse;
        message.error(err.errors.join('；'));
      } else {
        message.success(`上传成功，快照 ID: ${result.config_snapshot_id}`);
        onClose();
      }
    } catch {
      message.error('上传失败');
    }
    return false;
  };

  return (
    <Modal title="上传配置快照" open={open} footer={null} onCancel={onClose} width={600}>
      <Alert
        style={{ marginBottom: 16 }}
        type="info"
        message="上传 .zip 格式的配置文件包"
        description="必需文件：source.toml、mapping_dx.ini、load.toml、rules/ 目录"
      />
      <Dragger
        accept=".zip"
        multiple={false}
        showUploadList={false}
        beforeUpload={handleUpload}
        disabled={mutation.isPending}
      >
        <p className="ant-upload-drag-icon"><InboxOutlined /></p>
        <p className="ant-upload-text">点击或拖拽 zip 文件到此区域</p>
        <p className="ant-upload-hint">仅支持 .zip 格式</p>
      </Dragger>
      {mutation.isPending && <Alert style={{ marginTop: 16 }} type="warning" message="正在上传并校验..." />}
    </Modal>
  );
}
