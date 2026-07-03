import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listSnapshots, getSnapshot, uploadSnapshot, activateSnapshot } from './config-snapshots';
import { fetchGrid } from './results';
import { listTasks, dispatchTask } from './tasks';
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

export function useTasks() {
  return useQuery({
    queryKey: ['tasks'],
    queryFn: listTasks,
    refetchInterval: 30_000,
  });
}

export function useDispatchTask() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: TaskDispatchRequest) => dispatchTask(data),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['tasks'] });
    },
  });
}

export function useAgents() {
  return useQuery({
    queryKey: ['agents'],
    queryFn: listAgents,
    refetchInterval: 15_000,
  });
}
