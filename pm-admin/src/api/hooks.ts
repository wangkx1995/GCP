import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listSnapshots, getSnapshot, uploadSnapshot, activateSnapshot } from './config-snapshots';
import { fetchGrid } from './results';
import { dispatchTask } from './tasks';
import { listAgents, getAgentList } from './agents';
import type { GridQuery, TaskDispatchRequest } from '../types/api';

export function useSnapshots() {
  return useQuery({
    queryKey: ['config-snapshots'],
    queryFn: listSnapshots,
    refetchInterval: 30_000,
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

export function useGrid(query: GridQuery) {
  return useQuery({
    queryKey: ['grid', query],
    queryFn: () => fetchGrid(query),
    enabled: !!query.strategy_id && !!query.day,
    refetchInterval: 60_000,
  });
}

export function useDispatchTask() {
  return useMutation({
    mutationFn: (data: TaskDispatchRequest) => dispatchTask(data),
  });
}

export function useAgents() {
  return useQuery({
    queryKey: ['agents'],
    queryFn: listAgents,
    refetchInterval: 30_000,
  });
}

export function useAgentList() {
  return useQuery({
    queryKey: ['agent-list'],
    queryFn: getAgentList,
    refetchInterval: 30_000,
  });
}

import {
  listDataCollectorUnits,
  nextUnitId,
  saveDataCollectorUnit,
  deleteDataCollectorUnit,
  searchConfigNames,
  getTablesForConfig,
} from './data-collector-units';
import type { DataCollectorUnitSaveRequest } from '../types/api';

export function useDataCollectorUnits() {
  return useQuery({
    queryKey: ['data-collector-units'],
    queryFn: listDataCollectorUnits,
    refetchInterval: 30_000,
  });
}

export function useNextUnitId() {
  return useMutation({
    mutationFn: nextUnitId,
  });
}

export function useSaveDataCollectorUnit() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: DataCollectorUnitSaveRequest }) =>
      saveDataCollectorUnit(id, data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['data-collector-units'] });
    },
  });
}

export function useDeleteDataCollectorUnit() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: deleteDataCollectorUnit,
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['data-collector-units'] });
    },
  });
}

export function useConfigNames(search: string | undefined) {
  return useQuery({
    queryKey: ['config-names', search],
    queryFn: () => searchConfigNames(search),
    enabled: search !== undefined,
    staleTime: 60_000,
  });
}

export function useTablesForConfig(configName: string | undefined) {
  return useQuery({
    queryKey: ['config-tables', configName],
    queryFn: () => getTablesForConfig(configName!),
    enabled: !!configName,
    staleTime: 60_000,
  });
}

import { strategyApi } from './strategies';
import type {
  CollectionStrategyCreateRequest,
  CollectionStrategyUpdateRequest,
} from '../types/api';

export const useStrategies = (params?: { collector_name?: string; type?: string; status?: string }) =>
  useQuery({
    queryKey: ['strategies', params],
    queryFn: () => strategyApi.list(params),
    refetchInterval: 30_000,
  });

export const useStrategy = (id: string | null) =>
  useQuery({
    queryKey: ['strategy', id],
    queryFn: () => strategyApi.get(id!),
    enabled: id !== null,
  });

export const useCreateStrategies = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CollectionStrategyCreateRequest) => strategyApi.create(data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

export const useUpdateStrategy = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: CollectionStrategyUpdateRequest }) =>
      strategyApi.update(id, data),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

export const useBatchSuspend = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => strategyApi.batchSuspend(ids),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

export const useBatchActivate = () => {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => strategyApi.batchActivate(ids),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['strategies'] }); },
  });
};

import {
  getAgentDetail,
  updateAgent,
  getAgentStatusList,
  getAgentStatusHistory,
  getAgentGroupList,
  createAgentGroup,
  updateAgentGroup,
  deleteAgentGroup,
} from './agents';
import type {
  UpdateAgentRequest,
  CreateGroupRequest,
  UpdateGroupRequest,
} from '../types/api';

export function useAgentDetail(id: number | null) {
  return useQuery({
    queryKey: ['agent', id],
    queryFn: () => getAgentDetail(id!),
    enabled: id !== null,
  });
}

export function useUpdateAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateAgentRequest }) => updateAgent(id, data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['agents'] });
    },
  });
}

export function useAgentStatusList() {
  return useQuery({
    queryKey: ['agent-status-list'],
    queryFn: getAgentStatusList,
    refetchInterval: 30_000,
  });
}

export function useAgentStatusHistory(id: number | null, limit = 100) {
  return useQuery({
    queryKey: ['agent-status-history', id, limit],
    queryFn: () => getAgentStatusHistory(id!, limit),
    enabled: id !== null,
  });
}

export function useAgentGroupList() {
  return useQuery({
    queryKey: ['agent-groups'],
    queryFn: getAgentGroupList,
    refetchInterval: 30_000,
  });
}

export function useCreateAgentGroup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateGroupRequest) => createAgentGroup(data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['agent-groups'] });
    },
  });
}

export function useUpdateAgentGroup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateGroupRequest }) => updateAgentGroup(id, data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['agent-groups'] });
    },
  });
}

export function useDeleteAgentGroup() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteAgentGroup(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['agent-groups'] });
    },
  });
}


