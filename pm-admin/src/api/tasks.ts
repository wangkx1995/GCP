import http from './client';
import type { TaskDispatchRequest, TaskDispatchResponse } from '../types/api';

export function dispatchTask(data: TaskDispatchRequest) {
  return http.post<TaskDispatchResponse>('/tasks/dispatch', data).then(r => r.data);
}
