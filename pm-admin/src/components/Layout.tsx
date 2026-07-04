import { Layout as AntLayout, Menu } from 'antd';
import { Outlet, useNavigate, useLocation } from 'react-router-dom';
import {
  FileZipOutlined,
  DesktopOutlined,
  AppstoreOutlined,
  BarChartOutlined,
  SendOutlined,
  SettingOutlined,
} from '@ant-design/icons';

const { Sider, Content } = AntLayout;

const menuItems = [
  { key: '/config-snapshots', icon: <FileZipOutlined />, label: '采集适配器管理' },
  { key: '/agents', icon: <DesktopOutlined />, label: '采集机管理' },
  { key: '/data-collector-units', icon: <SettingOutlined />, label: '采集单元配置' },
  { key: '/strategy-dispatch', icon: <SendOutlined />, label: '采集策略下发' },
  { key: '/tasks', icon: <AppstoreOutlined />, label: '任务列表' },
  { key: '/results/grid', icon: <BarChartOutlined />, label: '结果网格' },
];

export default function Layout() {
  const navigate = useNavigate();
  const location = useLocation();

  return (
    <AntLayout style={{ minHeight: '100vh', background: '#F1F5F9' }}>
      <Sider
        width={220}
        theme="light"
        style={{
          background: '#FFFFFF',
          borderRight: '1px solid #E2E8F0',
        }}
      >
        <div className="sidebar-logo">
          <div className="sidebar-logo-icon">PM</div>
          <span className="sidebar-logo-text">PM Admin</span>
        </div>
        <Menu
          theme="light"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
          style={{
            background: 'transparent',
            borderInlineEnd: 'none',
            paddingTop: 8,
          }}
        />
      </Sider>
      <Content style={{ padding: 32, background: '#F1F5F9' }}>
        <Outlet />
      </Content>
    </AntLayout>
  );
}
