import { HashRouter, Navigate, Route, Routes } from 'react-router-dom';
import AppShell from './components/Layout/AppShell';
import Dashboard from './pages/Dashboard';
import ConfigPage from './pages/Config';
import ConfigDetailPage from './pages/ConfigDetail';
import DnsPage from './pages/Dns';
import LogsPage from './pages/Logs';
import ConnectionsPage from './pages/Connections';
import ProxiesPage from './pages/Proxies';
import RulesPage from './pages/Rules';
import SettingsPage from './pages/Settings';

export default function AppRoutes() {
  return (
    <HashRouter>
      <Routes>
        <Route element={<AppShell />}>
          <Route index element={<Dashboard />} />
          <Route path="config" element={<ConfigPage />} />
          <Route path="config/:id" element={<ConfigDetailPage />} />
          <Route path="proxies" element={<ProxiesPage />} />
          <Route path="route" element={<RulesPage />} />
          {/* Back-compat: anything still linking to /rules redirects. */}
          <Route path="rules" element={<Navigate to="/route" replace />} />
          <Route path="dns" element={<DnsPage />} />
          <Route path="logs" element={<LogsPage />} />
          <Route path="connections" element={<ConnectionsPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
