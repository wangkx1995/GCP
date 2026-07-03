import http from './client';
import type { AgentRegisterRequest, AgentRegisterResponse } from '../types/api';

export function registerAgent(data: AgentRegisterRequest) {
  return http.post<AgentRegisterResponse>('/agents/register', data).then(r => r.data);
}
