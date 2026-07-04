import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listSnapshots, getSnapshot, uploadSnapshot, activateSnapshot } from './config-snapshots';
import { fetchGrid } from './results';
import { dispatchTask } from './tasks';
import { listAgents } from './agents';
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
    mutationFn: ({ id, data }: { id: number; data: DataCollectorUnitSaveRequest }) =>
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


