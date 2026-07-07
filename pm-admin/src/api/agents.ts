import http from './client';
import type {
  AgentInfo,
  AgentInfoRow,
  AgentRegisterRequest,
  AgentRegisterResponse,
  AgentStatusRow,
  AgentStatusHisRow,
  AgentGroupRow,
  UpdateAgentRequest,
  CreateGroupRequest,
  UpdateGroupRequest,
} from '../types/api';

export function listAgents() {
  return http.get<AgentInfo[]>('/agents').then(r => r.data);
}

export function registerAgent(data: AgentRegisterRequest) {
  return http.post<AgentRegisterResponse>('/agents/register', data).then(r => r.data);
}

export function getAgentList() {
  return http.get<AgentInfoRow[]>('/agents').then(r => r.data);
}

export function getAgentDetail(id: number) {
  return http.get<AgentInfoRow>(`/agents/${id}`).then(r => r.data);
}

export function updateAgent(id: string, data: UpdateAgentRequest) {
  return http.patch<AgentInfoRow>(`/agents/${id}`, data).then(r => r.data);
}

export function getAgentStatusList() {
  return http.get<AgentStatusRow[]>('/agents/status').then(r => r.data);
}

export function getAgentStatusHistory(id: number, limit = 100) {
  return http.get<AgentStatusHisRow[]>(`/agents/${id}/status-history`, { params: { limit } }).then(r => r.data);
}

export function getAgentGroupList() {
  return http.get<AgentGroupRow[]>('/agent-groups').then(r => r.data);
}

export function createAgentGroup(data: CreateGroupRequest) {
  return http.post<AgentGroupRow>('/agent-groups', data).then(r => r.data);
}

export function updateAgentGroup(id: string, data: UpdateGroupRequest) {
  return http.put<AgentGroupRow>(`/agent-groups/${id}`, data).then(r => r.data);
}

export function deleteAgentGroup(id: string) {
  return http.delete(`/agent-groups/${id}`).then(r => r.data);
}
