import http from './client';
import type { TaskDispatchRequest, TaskDispatchResponse } from '../types/api';

export function dispatchTask(data: TaskDispatchRequest) {
  return http.post<TaskDispatchResponse>('/tasks/dispatch', data).then(r => r.data);
}

export function listTasks() {
  return http.get<Array<{ task_id: string; strategy_id: string; status: string; created_at: string; updated_at: string }>>('/tasks').then(r => r.data);
}
