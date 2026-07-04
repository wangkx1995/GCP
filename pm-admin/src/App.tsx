import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import { theme } from './styles/theme';
import Layout from './components/Layout';
import ConfigSnapshotsPage from './pages/ConfigSnapshots';
import AgentsPage from './pages/Agents';
import ResultsPage from './pages/Results';
import TasksPage from './pages/Tasks';
import StrategyDispatchPage from './pages/StrategyDispatch';
import DataCollectorUnitsPage from './pages/DataCollectorUnits';
import DataCollectorUnitFormPage from './pages/DataCollectorUnits/FormPage';

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 2, staleTime: 10_000 },
  },
});

export default function App() {
  return (
    <ConfigProvider locale={zhCN} theme={theme}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/" element={<Navigate to="/config-snapshots" />} />
              <Route path="/config-snapshots" element={<ConfigSnapshotsPage />} />
              <Route path="/agents" element={<AgentsPage />} />
              <Route path="/data-collector-units" element={<DataCollectorUnitsPage />} />
              <Route path="/data-collector-units/new" element={<DataCollectorUnitFormPage />} />
              <Route path="/data-collector-units/:id" element={<DataCollectorUnitFormPage />} />
              <Route path="/tasks" element={<TasksPage />} />
              <Route path="/strategy-dispatch" element={<StrategyDispatchPage />} />
              <Route path="/results/grid" element={<ResultsPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
