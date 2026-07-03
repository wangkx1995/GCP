import http from './client';
import type { DailyGrid, GridQuery } from '../types/api';

export function fetchGrid(query: GridQuery) {
  return http.get<DailyGrid>('/results/grid', { params: query }).then(r => r.data);
}
