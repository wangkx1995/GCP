import type { ThemeConfig } from 'antd';

export const theme: ThemeConfig = {
  token: {
    colorPrimary: '#22C55E',
    colorSuccess: '#22C55E',
    colorWarning: '#F59E0B',
    colorError: '#EF4444',
    colorInfo: '#22C55E',
    colorBgLayout: '#020617',
    colorBgContainer: '#0F172A',
    colorBgElevated: '#1A1E2F',
    colorText: '#F8FAFC',
    colorTextSecondary: '#94A3B8',
    colorBorder: '#334155',
    colorBorderSecondary: '#1E293B',
    borderRadius: 8,
    fontFamily: "'Fira Sans', -apple-system, BlinkMacSystemFont, sans-serif",
    fontSize: 14,
    controlHeight: 36,
  },
  components: {
    Menu: {
      colorItemBg: 'transparent',
      colorItemBgHover: '#1A1E2F',
      colorItemBgSelected: '#1A1E2F',
      colorItemText: '#64748B',
      colorItemTextHover: '#F8FAFC',
      colorItemTextSelected: '#22C55E',
      colorActiveBarWidth: 3,
      colorActiveBarHeight: 20,
      colorActiveBarBorderSize: 0,
    },
    Table: {
      headerBg: '#0F172A',
      headerColor: '#64748B',
      headerSortActiveBg: '#1A1E2F',
      headerSortHoverBg: '#1A1E2F',
      rowHoverBg: 'rgba(34, 197, 94, 0.04)',
      borderColor: '#1E293B',
      fontSize: 13,
    },
    Card: {
      colorBorderSecondary: '#1E293B',
    },
    Button: {
      primaryShadow: 'none',
      primaryColor: '#020617',
    },
    Tag: {
      fontSize: 12,
    },
    Modal: {
      contentBg: '#1A1E2F',
      headerBg: '#1A1E2F',
    },
    Drawer: {
      colorBgElevated: '#1A1E2F',
    },
    Select: {
      optionSelectedBg: 'rgba(34, 197, 94, 0.12)',
    },
    DatePicker: {
      cellHoverBg: 'rgba(34, 197, 94, 0.08)',
    },
    Message: {
      contentBg: '#1A1E2F',
    },
    Notification: {
      colorBgElevated: '#1A1E2F',
    },
  },
};
