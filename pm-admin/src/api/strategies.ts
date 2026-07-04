import http from './client';
import type {
  CollectionStrategy,
  CollectionStrategyCreateRequest,
  CollectionStrategyUpdateRequest,
} from '../types/api';

export const strategyApi = {
  nextId: () =>
    http.post<{ id: number }>('/api/strategies/next-id', {}).then(r => r.data),

  list: (params?: { collector_name?: string; type?: string; status?: string }) =>
    http.get<CollectionStrategy[]>('/api/strategies', { params }).then(r => r.data),

  get: (id: number) =>
    http.get<CollectionStrategy>(`/api/strategies/${id}`).then(r => r.data),

  create: (data: CollectionStrategyCreateRequest) =>
    http.post<CollectionStrategy[]>('/api/strategies', data).then(r => r.data),

  update: (id: number, data: CollectionStrategyUpdateRequest) =>
    http.put<Record<string, never>>(`/api/strategies/${id}`, data).then(r => r.data),

  batchSuspend: (ids: number[]) =>
    http.post<{ affected: number }>('/api/strategies/batch-suspend', { ids }).then(r => r.data),

  batchActivate: (ids: number[]) =>
    http.post<{ affected: number }>('/api/strategies/batch-activate', { ids }).then(r => r.data),
};
