import { Navigate, Route, Routes } from 'react-router-dom';

import DefaultRedirect from './components/DefaultRedirect';
import ProtectedRoute from './components/ProtectedRoute';
import PublicRoute from './components/PublicRoute';
import HumanPage from './features/human/HumanPage';
import Accounts from './pages/Accounts';
import Connections from './pages/Connections';
import Home from './pages/Home';
import Intelligence from './pages/Intelligence';
import Invites from './pages/Invites';
import Notifications from './pages/Notifications';
import Onboarding from './pages/onboarding/Onboarding';
import Rewards from './pages/Rewards';
import Settings from './pages/Settings';
import Welcome from './pages/Welcome';
import WorkflowsList from './pages/Workflows/WorkflowsList';

const AppRoutes = () => {
  return (
    <Routes>
      {/* Public routes - redirect to /home if logged in */}
      <Route
        path="/"
        element={
          <PublicRoute>
            <Welcome />
          </PublicRoute>
        }
      />

      {/* Onboarding (full-page stepper, gated by onboarding_completed) */}
      <Route
        path="/onboarding/*"
        element={
          <ProtectedRoute requireAuth={true}>
            <Onboarding />
          </ProtectedRoute>
        }
      />

      {/* Protected routes */}
      <Route
        path="/home"
        element={
          <ProtectedRoute requireAuth={true}>
            <Home />
          </ProtectedRoute>
        }
      />

      <Route
        path="/human"
        element={
          <ProtectedRoute requireAuth={true}>
            <HumanPage />
          </ProtectedRoute>
        }
      />

      <Route
        path="/intelligence"
        element={
          <ProtectedRoute requireAuth={true}>
            <Intelligence />
          </ProtectedRoute>
        }
      />

      {/* /skills and /channels were unified into /connections in Phase 0
          of Workflows & Automations (see Automations/prd.md §5). The legacy
          paths redirect; HashRouter preserves muscle memory + deep links. */}
      <Route
        path="/connections"
        element={
          <ProtectedRoute requireAuth={true}>
            <Connections />
          </ProtectedRoute>
        }
      />
      <Route path="/skills" element={<Navigate to="/connections" replace />} />

      {/* Workflows & Automations Phase 1 (F-4). Lives between
          /connections and /intelligence in the bottom-tab order. */}
      <Route
        path="/workflows"
        element={
          <ProtectedRoute requireAuth={true}>
            <WorkflowsList />
          </ProtectedRoute>
        }
      />

      {/* Unified chat = agent + connected web apps. Replaces the old
          /conversations and /accounts routes. */}
      <Route
        path="/chat"
        element={
          <ProtectedRoute requireAuth={true}>
            <Accounts />
          </ProtectedRoute>
        }
      />

      <Route path="/channels" element={<Navigate to="/connections#channels" replace />} />

      <Route
        path="/invites"
        element={
          <ProtectedRoute requireAuth={true}>
            <Invites />
          </ProtectedRoute>
        }
      />

      <Route
        path="/notifications"
        element={
          <ProtectedRoute requireAuth={true}>
            <Notifications />
          </ProtectedRoute>
        }
      />

      <Route
        path="/rewards"
        element={
          <ProtectedRoute requireAuth={true}>
            <Rewards />
          </ProtectedRoute>
        }
      />

      <Route path="/webhooks" element={<Navigate to="/settings/webhooks-triggers" replace />} />

      <Route
        path="/settings/*"
        element={
          <ProtectedRoute requireAuth={true}>
            <Settings />
          </ProtectedRoute>
        }
      />

      {/* Default redirect based on auth status */}
      <Route path="*" element={<DefaultRedirect />} />
    </Routes>
  );
};

export default AppRoutes;
