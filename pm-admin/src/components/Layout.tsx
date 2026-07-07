import { Layout as AntLayout, Menu } from 'antd';
import { Outlet, useNavigate, useLocation } from 'react-router-dom';
import {
  FileZipOutlined,
  DesktopOutlined,
  AppstoreOutlined,
  BarChartOutlined,
  SendOutlined,
  SettingOutlined,
  InfoCircleOutlined,
  ThunderboltOutlined,
  SyncOutlined,
} from '@ant-design/icons';

const { Sider, Content } = AntLayout;

const menuItems = [
  { key: '/config-snapshots', icon: <FileZipOutlined />, label: '采集适配器管理' },
  {
    key: 'agents-sub', icon: <DesktopOutlined />, label: '采集机管理',
    children: [
      { key: '/agents', icon: <InfoCircleOutlined />, label: '采集机信息' },
      { key: '/agents/status', icon: <ThunderboltOutlined />, label: '负载均衡' },
      { key: '/agents/history', icon: <BarChartOutlined />, label: '状态历史' },
      { key: '/agent-groups', icon: <SettingOutlined />, label: '采集机组' },
    ],
  },
  { key: '/data-collector-units', icon: <SettingOutlined />, label: '采集单元管理' },
  {
    key: '/strategy-dispatch', icon: <SendOutlined />, label: '采集策略管理',
    children: [
      { key: '/strategy-dispatch/info', icon: <InfoCircleOutlined />, label: '采集策略信息' },
      { key: '/strategy-dispatch/immediate', icon: <ThunderboltOutlined />, label: '及时采集策略' },
      { key: '/strategy-dispatch/periodic', icon: <SyncOutlined />, label: '周期性采集策略' },
    ],
  },
  { key: '/tasks', icon: <AppstoreOutlined />, label: '任务列表' },
  { key: '/results/grid', icon: <BarChartOutlined />, label: '结果网格' },
];

export default function Layout() {
  const navigate = useNavigate();
  const location = useLocation();

  const selectedKey = (() => {
    for (const item of menuItems) {
      if (item.children) {
        for (const child of item.children) {
          if (location.pathname === child.key) return child.key;
        }
      }
      if (location.pathname === item.key || location.pathname.startsWith(item.key + '/')) return item.key;
    }
    return location.pathname;
  })();

  const openKeys = (() => {
    for (const item of menuItems) {
      if (item.children) {
        for (const child of item.children) {
          if (location.pathname === child.key) return [item.key];
        }
      }
    }
    return [];
  })();

  return (
    <AntLayout style={{ height: '100vh', overflow: 'hidden', background: 'var(--color-bg-layout)' }}>
      <Sider
        width={220}
        theme="light"
        style={{
          background: 'var(--color-bg-container)',
          borderRight: '1px solid var(--color-border)',
          height: '100vh',
          position: 'sticky',
          top: 0,
          left: 0,
        }}
      >
        <div className="sidebar-logo">
          <div className="sidebar-logo-icon">PM</div>
          <span className="sidebar-logo-text">PM Admin</span>
        </div>
        <Menu
          theme="light"
          mode="inline"
          selectedKeys={[selectedKey]}
          defaultOpenKeys={openKeys}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
          style={{
            background: 'transparent',
            borderInlineEnd: 'none',
            paddingTop: 8,
          }}
        />
      </Sider>
      <Content style={{ padding: '32px 32px 0', background: 'var(--color-bg-layout)', height: '100vh', overflowY: 'auto' }}>
        <Outlet />
      </Content>
    </AntLayout>
  );
}
