import { Layout as AntLayout, Menu } from 'antd';
import { Outlet, useNavigate, useLocation } from 'react-router-dom';
import {
  FileZipOutlined,
  DesktopOutlined,
  AppstoreOutlined,
  BarChartOutlined,
} from '@ant-design/icons';

const { Sider, Content } = AntLayout;

const menuItems = [
  { key: '/config-snapshots', icon: <FileZipOutlined />, label: '配置快照' },
  { key: '/agents', icon: <DesktopOutlined />, label: 'Agent 管理' },
  { key: '/tasks', icon: <AppstoreOutlined />, label: '任务列表' },
  { key: '/results/grid', icon: <BarChartOutlined />, label: '结果网格' },
];

export default function Layout() {
  const navigate = useNavigate();
  const location = useLocation();

  return (
    <AntLayout style={{ minHeight: '100vh', background: '#020617' }}>
      <Sider
        width={220}
        style={{
          background: '#0F172A',
          borderRight: '1px solid rgba(255,255,255,0.06)',
        }}
      >
        <div className="sidebar-logo">
          <div className="sidebar-logo-icon">PM</div>
          <span className="sidebar-logo-text">PM Admin</span>
        </div>
        <Menu
          theme="dark"
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
      <Content style={{ padding: 32, background: '#020617' }}>
        <Outlet />
      </Content>
    </AntLayout>
  );
}
