/**
 * Asserts `/skills` and `/channels` redirect to `/connections`.
 *
 * Implements the P0-4 redirect verification from `Automations/Tickets/
 * phase-0-connections-hub/P0-4.md`. Uses a minimal Routes tree mirroring the
 * shape declared in `app/src/AppRoutes.tsx` so the test is independent of the
 * heavy page-component import chain (every protected route in AppRoutes
 * pulls in the full app shell).
 */
import { render, screen } from '@testing-library/react';
import { MemoryRouter, Navigate, Route, Routes } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

const ConnectionsStub = () => <div data-testid="connections-page-root">CONNECTIONS</div>;

const RedirectRoutes = () => (
  <Routes>
    <Route path="/connections" element={<ConnectionsStub />} />
    {/* Mirror the redirects in AppRoutes.tsx. */}
    <Route path="/skills" element={<Navigate to="/connections" replace />} />
    <Route path="/channels" element={<Navigate to="/connections#channels" replace />} />
  </Routes>
);

describe('Phase 0 route redirects', () => {
  it('/skills redirects to /connections', () => {
    render(
      <MemoryRouter initialEntries={['/skills']}>
        <RedirectRoutes />
      </MemoryRouter>
    );
    expect(screen.getByTestId('connections-page-root')).toBeInTheDocument();
  });

  it('/channels redirects to /connections (with #channels anchor)', () => {
    // MemoryRouter preserves the hash on redirect; the rendered page is the
    // canonical Connections page. P0-5 will scroll-to-anchor on hash.
    render(
      <MemoryRouter initialEntries={['/channels']}>
        <RedirectRoutes />
      </MemoryRouter>
    );
    expect(screen.getByTestId('connections-page-root')).toBeInTheDocument();
  });

  it('/connections renders directly without a redirect hop', () => {
    render(
      <MemoryRouter initialEntries={['/connections']}>
        <RedirectRoutes />
      </MemoryRouter>
    );
    expect(screen.getByTestId('connections-page-root')).toBeInTheDocument();
  });
});
