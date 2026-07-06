import http from './client';
import type { AgentInfo, AgentRegisterRequest, AgentRegisterResponse } from '../types/api';

export function listAgents() {
  return http.get<AgentInfo[]>('/agents').then(r => r.data);
}

export function registerAgent(data: AgentRegisterRequest) {
  return http.post<AgentRegisterResponse>('/agents/register', data).then(r => r.data);
}
