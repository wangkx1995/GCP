# 前端开发指南

## 技术栈

| 技术 | 版本 | 用途 |
|------|------|------|
| React | 18 | 框架 |
| TypeScript | 5 | 类型安全 |
| Vite | 5 | 构建工具 |
| Ant Design | 5 | UI 组件库 |
| TanStack Query | 5 | 服务端状态管理 |
| React Router | 6 | 路由 |
| dayjs | — | 日期处理（AntD 内置依赖） |
| fetch | — | HTTP 客户端 |

## 快速开始

```bash
# 创建项目
npm create vite@latest pm-admin -- --template react-ts
cd pm-admin

# 安装依赖
npm install antd @ant-design/icons @tanstack/react-query react-router-dom dayjs axios

# 启动
npm run dev
```

## 目录结构

```
src/
├── api/                  # API 调用 + TanStack Query hooks
│   ├── client.ts         # HTTP 客户端（fetch 封装）
│   ├── config-snapshots.ts
│   ├── agents.ts
│   ├── tasks.ts
│   └── results.ts
├── types/                # TypeScript 类型定义
│   ├── api.ts            # API 请求/响应类型
│   └── enums.ts          # 枚举常量
├── pages/                # 页面组件
│   ├── ConfigSnapshots/
│   │   ├── index.tsx           # 配置快照列表页
│   │   ├── UploadModal.tsx     # 上传弹窗
│   │   └── ActivateButton.tsx  # 激活按钮
│   ├── Agents/
│   │   └── index.tsx           # Agent 列表页
│   ├── Results/
│   │   ├── index.tsx           # 结果网格页
│   │   └── GridTable.tsx       # 网格渲染组件
│   └── Tasks/
│       └── index.tsx           # 任务列表页
├── components/           # 通用组件
│   ├── Layout.tsx        # 页面布局（侧边栏 + 内容区）
│   └── StatusTag.tsx     # 状态标签（颜色标记）
├── hooks/                # 自定义 hooks
│   └── useApi.ts         # fetch 封装
├── App.tsx               # 路由配置
├── main.tsx              # 入口（Provider 配置）
└── vite-env.d.ts
```

## 类型定义

### `src/types/api.ts`

```typescript
// ========== 配置快照 ==========

export interface ConfigSnapshotMeta {
  config_snapshot_id: string;
  content_hash: string;
  version_label: string | null;
  is_active: boolean;
  file_count: number;
  created_at: string;  // "YYYY-MM-DD HH:mm:ss"
  activated_at: string | null;
}

export interface UploadSuccessResponse {
  valid: true;
  config_snapshot_id: string;
  content_hash: string;
  file_count: number;
}

export interface UploadErrorResponse {
  valid: false;
  errors: string[];
  config_snapshot_id: string;
}

export type UploadResponse = UploadSuccessResponse | UploadErrorResponse;

export interface ActivateResponse {
  config_snapshot_id: string;
  active: true;
  content_hash: string;
  activated_at: string | null;
}

// ========== Agent ==========

export interface AgentCapabilities {
  can_collect: boolean;
  can_parse: boolean;
  can_load: boolean;
  supported_protocols: string[];
}

export interface AgentRegisterRequest {
  agent_id: string | null;
  agent_name: string;
  host: string;
  port: number;
  version: string;
  capabilities: AgentCapabilities;
}

export interface AgentRegisterResponse {
  agent_id: string;
  heartbeat_interval_seconds: number;
  task_report_interval_seconds: number;
}

// ========== 任务 ==========

export interface TaskDispatchRequest {
  task_id: string;
  logical_task_key: string;
  strategy_id: string;
  config_snapshot_id: string;
  scan_start_time: string;
  collect_id: string;
  load_type: string;
  encoding: string;
  output_delimiter: string;
  timeout_seconds: number;
  callback_base_url: string;
}

export interface TaskDispatchResponse {
  task_id: string;
  accepted: boolean;
  agent_task_state: TaskStatus;
  reason: string | null;
}

export interface ResultRow {
  table_name: string;
  data_time: string;
  row_count: number;
  success: number;   // 0 or 1
  collect_time: string;
}

export interface TaskResultReport {
  task_id: string;
  agent_id: string;
  status: TaskStatus;
  result_rows: ResultRow[];
}

// ========== 结果网格 ==========

export interface DailyGrid {
  day: string;
  time_slots: string[];
  rows: TableGridRow[];
}

export interface TableGridRow {
  table_name: string;
  cells: GridCell[];
}

export interface GridCell {
  data_time: string;
  value: number | null;
  color: 'green' | 'yellow' | 'red' | 'gray';
  status: 'ok' | 'empty' | 'failed' | 'missing';
}

export interface GridQuery {
  strategy_id: string;
  day: string;    // "YYYY-MM-DD"
  interval_minutes?: number;  // 默认 15
}

// ========== 配置更新通知 ==========

export interface ConfigUpdateRequest {
  snapshot_id: string;
  content_hash: string;
}
```

### `src/types/enums.ts`

```typescript
export enum TaskStatus {
  CREATED = 'CREATED',
  DISPATCHING = 'DISPATCHING',
  ACCEPTED = 'ACCEPTED',
  RUNNING = 'RUNNING',
  SUCCEEDED = 'SUCCEEDED',
  FAILED = 'FAILED',
  TIMEOUT = 'TIMEOUT',
  CANCEL_REQUESTED = 'CANCEL_REQUESTED',
  CANCELLED = 'CANCELLED',
}

export const GRID_COLORS: Record<string, { color: string; label: string }> = {
  green:  { color: '#52c41a', label: '正常' },
  yellow: { color: '#faad14', label: '空数据' },
  red:    { color: '#ff4d4f', label: '失败' },
  gray:   { color: '#d9d9d9', label: '缺失' },
};
```

## API 层

### `src/api/client.ts`

```typescript
import axios from 'axios';

const http = axios.create({
  baseURL: import.meta.env.VITE_CORE_API_BASE || 'http://127.0.0.1:18080/api',
  timeout: 30_000,
  headers: { 'Content-Type': 'application/json' },
});

http.interceptors.response.use(
  res => res,
  error => {
    const msg = error.response?.data
      ? JSON.stringify(error.response.data)
      : error.message;
    return Promise.reject(new Error(msg));
  },
);

export default http;
```

### `src/api/config-snapshots.ts`

```typescript
import http from './client';
import type {
  ConfigSnapshotMeta,
  UploadResponse,
  ActivateResponse,
} from '../types/api';

export function listSnapshots() {
  return http.get<ConfigSnapshotMeta[]>('/config-snapshots').then(r => r.data);
}

export function getSnapshot(id: string) {
  return http.get<ConfigSnapshotMeta>(`/config-snapshots/${id}`).then(r => r.data);
}

export async function uploadSnapshot(file: File): Promise<UploadResponse> {
  const res = await http.post('/config-snapshots/upload', await file.arrayBuffer(), {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
  return res.data;
}

export function activateSnapshot(id: string) {
  return http.post<ActivateResponse>(`/config-snapshots/${id}/activate`).then(r => r.data);
}

/** 触发浏览器下载 */
export function downloadSnapshot(id: string) {
  window.open(`${http.defaults.baseURL}/config-snapshots/${id}/download`, '_blank');
}
```

### `src/api/results.ts`

```typescript
import http from './client';
import type { DailyGrid, GridQuery } from '../types/api';

export function fetchGrid(query: GridQuery) {
  return http.get<DailyGrid>('/results/grid', { params: query }).then(r => r.data);
}
```

## TanStack Query Hooks

### `src/api/config-snapshots.ts`（补充 hooks）

```typescript
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listSnapshots, uploadSnapshot, activateSnapshot } from './config-snapshots';

export function useSnapshots() {
  return useQuery({
    queryKey: ['config-snapshots'],
    queryFn: listSnapshots,
    refetchInterval: 30_000,  // 30s 自动刷新
  });
}

export function useSnapshot(id: string) {
  return useQuery({
    queryKey: ['config-snapshots', id],
    queryFn: () => getSnapshot(id),
    enabled: !!id,
  });
}

export function useUploadSnapshot() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: uploadSnapshot,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['config-snapshots'] });
    },
  });
}

export function useActivateSnapshot() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: activateSnapshot,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['config-snapshots'] });
    },
  });
}
```

### `src/api/results.ts`（补充 hooks）

```typescript
import { useQuery } from '@tanstack/react-query';
import { fetchGrid } from './results';
import type { GridQuery } from '../types/api';

export function useGrid(query: GridQuery) {
  return useQuery({
    queryKey: ['grid', query],
    queryFn: () => fetchGrid(query),
    enabled: !!query.strategy_id && !!query.day,
    refetchInterval: 60_000,
  });
}
```

## 路由配置

### `src/App.tsx`

```tsx
import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import Layout from './components/Layout';
import ConfigSnapshotsPage from './pages/ConfigSnapshots';
import AgentsPage from './pages/Agents';
import ResultsPage from './pages/Results';
import TasksPage from './pages/Tasks';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 2, staleTime: 10_000 },
  },
});

export default function App() {
  return (
    <ConfigProvider locale={zhCN}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/" element={<Navigate to="/config-snapshots" />} />
              <Route path="/config-snapshots" element={<ConfigSnapshotsPage />} />
              <Route path="/agents" element={<AgentsPage />} />
              <Route path="/tasks" element={<TasksPage />} />
              <Route path="/results/grid" element={<ResultsPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
```

## 页面组件开发指南

### 配置快照列表页

```tsx
// src/pages/ConfigSnapshots/index.tsx
import { Table, Button, Space, Tag, message, Modal } from 'antd';
import { UploadOutlined, DownloadOutlined, CheckCircleOutlined } from '@ant-design/icons';
import { useSnapshots, useActivateSnapshot } from '../../api/config-snapshots';
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
```

### 上传弹窗

```tsx
// src/pages/ConfigSnapshots/UploadModal.tsx
import { Modal, Upload, Alert, message } from 'antd';
import { InboxOutlined } from '@ant-design/icons';
import { useUploadSnapshot } from '../../api/config-snapshots';
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
    return false; // 阻止默认上传行为
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
```

### 结果网格页

```tsx
// src/pages/Results/index.tsx
import { useState } from 'react';
import { Card, Select, DatePicker, Radio } from 'antd';
import dayjs from 'dayjs';
import GridTable from './GridTable';
import { useGrid } from '../../api/results';

const STRATEGIES = ['strategy_1', 'strategy_2']; // 可扩展为从 API 拉取

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
```

### 网格表格渲染

```tsx
// src/pages/Results/GridTable.tsx
import { Table, Tooltip } from 'antd';
import { GRID_COLORS } from '../../types/enums';
import type { DailyGrid, GridCell } from '../../types/api';

interface Props {
  grid: DailyGrid;
  loading: boolean;
}

export default function GridTable({ grid, loading }: Props) {
  // 列定义：时间槽列 + 固定表名列
  const columns = [
    {
      title: '表名',
      dataIndex: 'table_name',
      key: 'table_name',
      fixed: 'left' as const,
      width: 120,
    },
    ...grid.time_slots.map(slot => ({
      title: slot.slice(11, 16),          // "15:15"
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
```

### 布局组件

```tsx
// src/components/Layout.tsx
import { Layout as AntLayout, Menu } from 'antd';
import { Outlet, useNavigate, useLocation } from 'react-router-dom';
import {
  FileZipOutlined,
  DesktopOutlined,
  AppstoreOutlined,
  BarChartOutlined,
} from '@ant-design/icons';

const { Sider, Content } = AntLayout;

const menuItems = [
  { key: '/config-snapshots', icon: <FileZipOutlined />, label: '配置快照' },
  { key: '/agents', icon: <DesktopOutlined />, label: 'Agent 管理' },
  { key: '/tasks', icon: <AppstoreOutlined />, label: '任务列表' },
  { key: '/results/grid', icon: <BarChartOutlined />, label: '结果网格' },
];

export default function Layout() {
  const navigate = useNavigate();
  const location = useLocation();

  return (
    <AntLayout style={{ minHeight: '100vh' }}>
      <Sider>
        <div style={{ color: '#fff', padding: 16, fontWeight: 'bold', fontSize: 16 }}>
          PM Admin
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
        />
      </Sider>
      <Content style={{ padding: 24 }}>
        <Outlet />
      </Content>
    </AntLayout>
  );
}
```

### 入口文件

```tsx
// src/main.tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
```

## 环境变量

### `.env`

```env
VITE_CORE_API_BASE=http://127.0.0.1:18080/api
```

### `.env.production`

```env
VITE_CORE_API_BASE=/api    # 生产环境通过 nginx 反向代理
```

## 开发备注

### 上传接口注意事项

`POST /api/config-snapshots/upload` 接收原始 zip 字节（非 multipart/form-data），请求头必须设为 `Content-Type: application/octet-stream`。上传时用 `file.arrayBuffer()` 获取文件内容：

```typescript
// 正确（axios）
await http.post('/config-snapshots/upload', await file.arrayBuffer(), {
  headers: { 'Content-Type': 'application/octet-stream' },
});

// 正确（fetch）
await fetch(url, {
  method: 'POST',
  headers: { 'Content-Type': 'application/octet-stream' },
  body: await file.arrayBuffer(),
});

// 错误 ❌ — 不要用 FormData
await http.post('/config-snapshots/upload', formData);
```

### 网格颜色规范

前端不必自行计算颜色 —— 服务端返回的 `GridCell.color` 字段已经包含颜色值。直接映射即可：

| color | 展示 | 说明 |
|-------|------|------|
| `green` | 绿色背景 + 行数 | 数据正常 |
| `yellow` | 黄色背景 + 0 | 表存在但无数据 |
| `red` | 红色背景 + 0 | 采集失败 |
| `gray` | 灰色背景 + '-' | 该时间槽无数据 |

### TanStack Query Devtools（开发用）

```tsx
// main.tsx 在开发环境启用
import { ReactQueryDevtools } from '@tanstack/react-query-devtools';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
      <ReactQueryDevtools initialIsOpen={false} />
    </QueryClientProvider>
  </React.StrictMode>
);
```

```bash
npm install -D @tanstack/react-query-devtools
```
