import http from './client';
import type {
  DataCollectorUnit,
  DataCollectorUnitSaveRequest,
  NextIdResponse,
  ConfigNamesResponse,
  TablesResponse,
} from '../types/api';

export function listDataCollectorUnits() {
  return http.get<DataCollectorUnit[]>('/data-collector-units').then(r => r.data);
}

export function nextUnitId() {
  return http.post<NextIdResponse>('/data-collector-units/next-id').then(r => r.data);
}

export function saveDataCollectorUnit(id: string, data: DataCollectorUnitSaveRequest) {
  return http.put<{ id: number }>(`/data-collector-units/${id}`, data).then(r => r.data);
}

export function deleteDataCollectorUnit(id: string) {
  return http.delete<{ deleted: boolean }>(`/data-collector-units/${id}`).then(r => r.data);
}

export function searchConfigNames(search?: string) {
  const params = search ? { search } : {};
  return http.get<ConfigNamesResponse>('/data-collector-units/config-names', { params }).then(r => r.data);
}

export function getTablesForConfig(config_name: string) {
  return http.get<TablesResponse>('/data-collector-units/tables', { params: { config_name } }).then(r => r.data);
}
