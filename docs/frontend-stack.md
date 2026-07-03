# 前端技术选型建议

## 应用画像

这是一个**内部管理后台**，核心功能：
- 配置快照管理（上传/列表/激活/下载）
- 结果网格查看（颜色标记的表格）
- Agent/任务管理

不需要 SEO、不需要 SSR、不需要复杂动画。

---

## 推荐方案

| 层 | 选型 | 理由 |
|----|------|------|
| 框架 | **React 18 + TypeScript** | 生态最大、组件库最丰富、招人容易 |
| 构建 | **Vite** | 秒级 HMR，零配置 |
| UI 组件 | **Ant Design 5** | 表格/表单/弹窗/上传开箱即用，中文文档完善 |
| 请求 | **TanStack Query (React Query)** | 缓存/重试/loading 状态自动管理，减少 60% 样板代码 |
| 路由 | **React Router 6** | 标准方案 |
| 图表 | **无需额外库** | 网格用 CSS + Ant Design Table 渲染颜色块即可 |
| HTTP | **fetch**（或 axios） | 简单场景 fetch 足够 |

---

## 方案对比

### 框架

| 维度 | React 18 | Vue 3 | Svelte 5 |
|------|----------|-------|----------|
| 组件库 | AntD 最成熟 | Element Plus 够用 | 选择少 |
| TypeScript | 极好 | 好 | 一般 |
| 国内生态 | 最大 | 较大，中文资料多 | 小 |
| 学习成本 | 中等 | 低 | 低 |
| 适用场景 | 中大型后台 | 中小型后台 | 轻量应用 |

**选 React 的原因：** Ant Design 的 Table/Form/Upload 组件刚好覆盖这个系统的全部需求，且 React Query 对"加载 → 展示 → 缓存 → 重试"这个循环的处理是业界最佳实践。

### UI 组件库

| 库 | 表格 | 上传 | 表单 | 国际化 |
|----|------|------|------|--------|
| **Ant Design 5** | ✅ 完美 | ✅ 完美 | ✅ 完美 | ✅ 内置中文 |
| shadcn/ui | ❌ 需手写 | ❌ 需手写 | ❌ 需手写 | ✅ |
| MUI | ⚠️ 繁重 | ⚠️ 一般 | ✅ | ✅ |

**Ant Design 优势：** 这个系统 70% 的页面是 "表格 + 表单" 模式，AntD 的 Table（筛选项、排序、列配置、展开行）和 Upload（拖拽上传、进度条）直接满足需求，不需要额外封装。

---

## 页面结构

```
/                          → 布局 + 侧边导航
├── /config-snapshots      → 配置快照列表（表格）
│   └── upload             → 上传 zip（拖拽上传组件）
├── /config-snapshots/:id  → 快照详情 + 激活
├── /agents                → Agent 列表
├── /tasks                 → 任务列表/历史
└── /results/grid          → 结果网格（策略 + 日期选择器 + 颜色表格）
```

---

## 核心组件清单

### 1. ConfigSnapshotsPage
- Ant Design `Table` 展示快照列表
- 每行操作：激活、下载
- 颜色标记当前激活的快照
- 顶部 "上传" 按钮

### 2. UploadModal
- Ant Design `Upload.Dragger` 拖拽上传
- `content-type: application/octet-stream` 上传 raw bytes
- 成功后自动刷新列表
- 失败时展示 AntD `Alert` 显示校验错误列表

### 3. ResultGridPage
- 策略选择器（AntD `Select`）+ 日期选择器（AntD `DatePicker`）
- 自定义颜色表格
- 颜色用 AntD `Tag` 组件渲染

### 4. AgentListPage
- Ant Design `Table` 展示 Agent 列表
- 状态颜色标记（ONLINE 绿色、OFFLINE 灰色）

---

## 数据流模式

用 TanStack Query 管理所有服务端状态：

```typescript
// 示例：配置列表
function useConfigSnapshots() {
  return useQuery({
    queryKey: ['config-snapshots'],
    queryFn: () => fetch('/api/config-snapshots').then(r => r.json()),
  })
}

// 示例：激活后自动刷新
function useActivateSnapshot() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      fetch(`/api/config-snapshots/${id}/activate`, { method: 'POST' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['config-snapshots'] })
    },
  })
}
```

**不需要 Redux/Zustand** 等全局状态管理 —— 所有数据来自服务端，TanStack Query 管缓存和同步。唯一需要的全局状态是当前选中的策略和日期，用 URL 参数即可。

---

## 预估工作量和文件

| 页面 | 组件 | 工作量 |
|------|------|--------|
| 配置列表 | `ConfigSnapshotsPage.tsx` | 1 天 |
| 上传弹窗 | `UploadModal.tsx` | 0.5 天 |
| 激活/下载 | 内联操作按钮 | 0.5 天 |
| Agent 列表 | `AgentListPage.tsx` | 0.5 天 |
| 结果网格 | `ResultGridPage.tsx` + `GridTable.tsx` | 1 天 |
| 布局/路由 | `App.tsx` + `Layout.tsx` | 0.5 天 |
| **合计** | | **~4 天** |

---

## 不推荐的方案

| 方案 | 不推荐理由 |
|------|-----------|
| Next.js / Nuxt | 不需要 SSR，加复杂度 |
| Redux / Zustand | 不需要全局状态管理，TanStack Query 足够 |
| Tailwind CSS | 与 Ant Design 搭配需要额外配置，无显著收益 |
| Monaco Editor / CodeMirror | 无代码编辑需求 |
| WebSocket | 当前没有实时推送需求，轮询即可 |
