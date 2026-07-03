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
    <AntLayout style={{ minHeight: '100vh' }}>
      <Sider>
        <div style={{ color: '#fff', padding: 16, fontWeight: 'bold', fontSize: 16 }}>
          PM Admin
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
        />
      </Sider>
      <Content style={{ padding: 24 }}>
        <Outlet />
      </Content>
    </AntLayout>
  );
}
