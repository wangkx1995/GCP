import http from './client';
import type { AgentRegisterRequest, AgentRegisterResponse } from '../types/api';

export function registerAgent(data: AgentRegisterRequest) {
  return http.post<AgentRegisterResponse>('/agents/register', data).then(r => r.data);
}

export function listAgents() {
  return http.get<Array<{ agent_id: string; agent_name: string; host: string; port: number; status: string; version: string; last_heartbeat: string | null }>>('/agents').then(r => r.data);
}
