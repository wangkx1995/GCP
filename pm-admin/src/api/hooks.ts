import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { listSnapshots, getSnapshot, uploadSnapshot, activateSnapshot } from './config-snapshots';
import { fetchGrid } from './results';
import { dispatchTask } from './tasks';
import { registerAgent } from './agents';
import type { GridQuery, TaskDispatchRequest, AgentRegisterRequest } from '../types/api';

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

export function useRegisterAgent() {
  return useMutation({
    mutationFn: (data: AgentRegisterRequest) => registerAgent(data),
  });
}
