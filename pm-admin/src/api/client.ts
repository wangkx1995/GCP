import axios from 'axios';

const http = axios.create({
  baseURL: import.meta.env.VITE_CORE_API_BASE || 'http://127.0.0.1:18080/api',
  timeout: 30_000,
  headers: { 'Content-Type': 'application/json' },
});

http.interceptors.response.use(
  res => res,
  error => {
    const msg = error.response?.data
      ? JSON.stringify(error.response.data)
      : error.message;
    return Promise.reject(new Error(msg));
  },
);

export default http;
