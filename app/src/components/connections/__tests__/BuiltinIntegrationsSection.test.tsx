/**
 * Tests for `<BuiltinIntegrationsSection>` (P0-6) — verifies the section
 * renders one card per fixture row, maps the `Builtin` ConnectionRef slug to
 * a stable testid, and surfaces the empty-state copy when no items are passed.
 */
import { render, screen } from '@testing-library/react';
import { type ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import type { ConnectionView } from '../../../types/connections';
import BuiltinIntegrationsSection from '../sections/BuiltinIntegrationsSection';

function renderInRouter(ui: ReactNode) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

function makeBuiltin(
  integration: string,
  displayName: string,
  status: ConnectionView['status'] = { kind: 'connected' }
): ConnectionView {
  return {
    ref: { type: 'builtin', integration },
    display_name: displayName,
    status,
    last_used_at: null,
    mechanism_label: 'Built-in',
  };
}

describe('<BuiltinIntegrationsSection>', () => {
  it('renders the empty-state copy when items is empty', () => {
    renderInRouter(<BuiltinIntegrationsSection items={[]} />);
    expect(screen.getByTestId('connections-section-builtin')).toBeInTheDocument();
    expect(screen.getByText(/Sign in to OpenHuman/i)).toBeInTheDocument();
  });

  it('renders one card per fixture row with stable testids per integration slug', () => {
    const items: ConnectionView[] = [
      makeBuiltin('twilio', 'Twilio'),
      makeBuiltin('apify', 'Apify'),
      makeBuiltin('google_places', 'Google Places'),
      makeBuiltin('parallel', 'Parallel'),
      makeBuiltin('seltz', 'Seltz'),
      makeBuiltin('stock_prices', 'Stock Prices'),
    ];

    renderInRouter(<BuiltinIntegrationsSection items={items} />);

    expect(screen.getByTestId('connection-card-builtin-twilio')).toBeInTheDocument();
    expect(screen.getByTestId('connection-card-builtin-apify')).toBeInTheDocument();
    expect(screen.getByTestId('connection-card-builtin-google_places')).toBeInTheDocument();
    expect(screen.getByTestId('connection-card-builtin-parallel')).toBeInTheDocument();
    expect(screen.getByTestId('connection-card-builtin-seltz')).toBeInTheDocument();
    expect(screen.getByTestId('connection-card-builtin-stock_prices')).toBeInTheDocument();
  });

  it('renders the per-integration description as the card subtitle', () => {
    renderInRouter(<BuiltinIntegrationsSection items={[makeBuiltin('twilio', 'Twilio')]} />);
    expect(screen.getByText(/SMS, voice calls/i)).toBeInTheDocument();
  });

  it('surfaces NotConnected status when no session token is present', () => {
    renderInRouter(
      <BuiltinIntegrationsSection
        items={[makeBuiltin('twilio', 'Twilio', { kind: 'not_connected' })]}
      />
    );
    expect(screen.getByText(/Not connected/i)).toBeInTheDocument();
  });
});
