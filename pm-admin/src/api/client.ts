import axios from 'axios';

const http = axios.create({
  baseURL: import.meta.env.VITE_CORE_API_BASE || '/api',
  timeout: 30_000,
  headers: { 'Content-Type': 'application/json' },
});

http.interceptors.response.use(
  res => {
    // Auto-unwrap ApiResponse: {data, status, message} -> inner data
    if (res.data && typeof res.data === 'object' && 'data' in res.data && 'status' in res.data && 'message' in res.data) {
      res.data = res.data.data;
    }
    return res;
  },
  error => {
    const body = error.response?.data;
    const msg = body && typeof body === 'object' && 'message' in body
      ? body.message
      : body
        ? JSON.stringify(body)
        : error.message;
    return Promise.reject(new Error(msg));
  },
);

export default http;
