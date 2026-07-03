import type { ThemeConfig } from 'antd';

export const theme: ThemeConfig = {
  token: {
    colorPrimary: '#0891b2',
    colorSuccess: '#10b981',
    colorWarning: '#f59e0b',
    colorError: '#ef4444',
    colorInfo: '#0891b2',
    colorBgLayout: '#f1f5f9',
    colorBgContainer: '#ffffff',
    colorText: '#1e293b',
    colorTextSecondary: '#64748b',
    colorBorder: '#e2e8f0',
    borderRadius: 8,
    fontFamily: "'Inter', -apple-system, BlinkMacSystemFont, sans-serif",
    fontSize: 14,
    controlHeight: 36,
  },
  components: {
    Menu: {
      colorItemBg: 'transparent',
      colorItemBgHover: '#1e293b',
      colorItemBgSelected: '#1e293b',
      colorItemText: '#94a3b8',
      colorItemTextHover: '#ffffff',
      colorItemTextSelected: '#ffffff',
      colorActiveBarWidth: 3,
      colorActiveBarHeight: 24,
      colorActiveBarBorderSize: 0,
    },
    Table: {
      headerBg: '#f8fafc',
      headerColor: '#64748b',
      headerSortActiveBg: '#f1f5f9',
      headerSortHoverBg: '#f1f5f9',
      rowHoverBg: '#f1f5f9',
      borderColor: '#f1f5f9',
      fontSize: 13,
    },
    Card: {
      colorBorderSecondary: '#e2e8f0',
    },
    Button: {
      primaryShadow: 'none',
    },
    Tag: {
      fontSize: 12,
    },
  },
};
