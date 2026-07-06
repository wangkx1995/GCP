import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import { theme } from './styles/theme';
import Layout from './components/Layout';
import ConfigSnapshotsPage from './pages/ConfigSnapshots';
import AgentsPage from './pages/Agents';
import AgentHistoryPage from './pages/Agents/HistoryPage';
import AgentStatusPage from './pages/Agents/StatusPage';
import AgentGroupsPage from './pages/AgentGroups';
import ResultsPage from './pages/Results';
import TasksPage from './pages/Tasks';
import StrategyInfoPage from './pages/StrategyDispatch/StrategyInfo';
import ImmediateStrategyPage from './pages/StrategyDispatch/ImmediateStrategy';
import PeriodicStrategyPage from './pages/StrategyDispatch/PeriodicStrategy';
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
              <Route path="/agents/history" element={<AgentHistoryPage />} />
              <Route path="/agents/status" element={<AgentStatusPage />} />
              <Route path="/agent-groups" element={<AgentGroupsPage />} />
              <Route path="/data-collector-units" element={<DataCollectorUnitsPage />} />
              <Route path="/data-collector-units/create" element={<DataCollectorUnitFormPage />} />
              <Route path="/data-collector-units/:id/edit" element={<DataCollectorUnitFormPage />} />
              <Route path="/tasks" element={<TasksPage />} />
              <Route path="/strategy-dispatch" element={<Navigate to="/strategy-dispatch/info" />} />
              <Route path="/strategy-dispatch/info" element={<StrategyInfoPage />} />
              <Route path="/strategy-dispatch/immediate" element={<ImmediateStrategyPage />} />
              <Route path="/strategy-dispatch/immediate/:id/edit" element={<ImmediateStrategyPage />} />
              <Route path="/strategy-dispatch/periodic" element={<PeriodicStrategyPage />} />
              <Route path="/strategy-dispatch/periodic/:id/edit" element={<PeriodicStrategyPage />} />
              <Route path="/results/grid" element={<ResultsPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
