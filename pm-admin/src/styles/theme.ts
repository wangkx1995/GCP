import type { ThemeConfig } from 'antd';

export const theme: ThemeConfig = {
  token: {
    colorPrimary: '#22C55E',
    colorSuccess: '#22C55E',
    colorWarning: '#F59E0B',
    colorError: '#EF4444',
    colorInfo: '#22C55E',
    colorBgLayout: '#F1F5F9',
    colorBgContainer: '#FFFFFF',
    colorBgElevated: '#FFFFFF',
    colorText: '#0F172A',
    colorTextSecondary: '#64748B',
    colorBorder: '#E2E8F0',
    colorBorderSecondary: '#F1F5F9',
    controlItemBgHover: '#F1F5F9',
    borderRadius: 8,
    fontFamily: "'Inter', -apple-system, BlinkMacSystemFont, sans-serif",
    fontSize: 14,
    controlHeight: 36,
  },
  components: {
    Menu: {
      itemBg: 'transparent',
      itemHoverBg: '#F1F5F9',
      itemSelectedBg: '#F0FDF4',
      itemColor: '#64748B',
      itemHoverColor: '#0F172A',
      itemSelectedColor: '#16A34A',
      activeBarWidth: 3,
      activeBarHeight: 20,
      colorActiveBarBorderSize: 0,
    },
    Table: {
      headerBg: '#F8FAFC',
      headerColor: '#64748B',
      headerSortActiveBg: '#F1F5F9',
      headerSortHoverBg: '#F1F5F9',
      rowHoverBg: '#F0FDF4',
      borderColor: '#F1F5F9',
      fontSize: 13,
    },
    Card: {
      colorBorderSecondary: '#E2E8F0',
    },
    Button: {
      primaryShadow: 'none',
      primaryColor: '#FFFFFF',
    },
    Tag: {
      fontSize: 12,
    },
    Modal: {
      contentBg: '#FFFFFF',
      headerBg: '#FFFFFF',
    },
    Drawer: {
      colorBgElevated: '#FFFFFF',
    },
    Select: {
      optionSelectedBg: '#F0FDF4',
      optionSelectedColor: '#16A34A',
    },
    Cascader: {
      optionSelectedBg: '#F0FDF4',
    },
    DatePicker: {
      cellHoverBg: '#F0FDF4',
    },
    Message: {
      contentBg: '#FFFFFF',
    },
    Notification: {
      colorBgElevated: '#FFFFFF',
    },
  },
};
