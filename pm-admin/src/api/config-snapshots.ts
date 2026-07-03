import http from './client';
import type { ConfigSnapshotMeta, UploadResponse, ActivateResponse } from '../types/api';

export function listSnapshots() {
  return http.get<ConfigSnapshotMeta[]>('/config-snapshots').then(r => r.data);
}

export function getSnapshot(id: string): Promise<ConfigSnapshotMeta | null> {
  return http.get<ConfigSnapshotMeta | null>(`/config-snapshots/${id}`).then(r => r.data);
}

export async function uploadSnapshot(file: File): Promise<UploadResponse> {
  const res = await http.post('/config-snapshots/upload', await file.arrayBuffer(), {
    headers: { 'Content-Type': 'application/octet-stream' },
  });
  return res.data;
}

export function activateSnapshot(id: string) {
  return http.post<ActivateResponse>(`/config-snapshots/${id}/activate`).then(r => r.data);
}

export function downloadSnapshot(id: string) {
  window.open(`${http.defaults.baseURL}/config-snapshots/${id}/download`, '_blank');
}
