import { describe, expect, it } from 'vitest';

import { getMascotPalette } from './mascotPalette';

describe('getMascotPalette', () => {
  it.each(['yellow', 'burgundy', 'black', 'navy', 'custom'] as const)(
    'returns a populated palette for %s',
    color => {
      const palette = getMascotPalette(color);
      expect(palette.bodyFill).toMatch(/^#[0-9A-Fa-f]{6}$/);
      expect(palette.armHighlightMatrix.split(/\s+/)).toHaveLength(20);
      expect(palette.armShadowMatrix.split(/\s+/)).toHaveLength(20);
      expect(palette.bodyHighlightMatrix.split(/\s+/)).toHaveLength(20);
      expect(palette.bodyShadowMatrix.split(/\s+/)).toHaveLength(20);
      expect(palette.headHighlightMatrix.split(/\s+/)).toHaveLength(20);
      expect(palette.headShadowMatrix.split(/\s+/)).toHaveLength(20);
      expect(palette.neckShadowColor).toMatch(/^#[0-9A-Fa-f]{6}$/);
    }
  );

  // Regression: post-app-update on 2026-05-27, the React error boundary
  // surfaced "TypeError: Cannot read properties of undefined (reading
  // 'bodyFill')" because a persisted-state mascotColor no longer matched
  // the supported set. `getMascotPalette` MUST fall back to the YELLOW
  // default rather than returning undefined for any unknown or
  // missing input.
  it.each([undefined, null, '', 'red', 'pastel', 'YELLOW', 0, false] as const)(
    'falls back to yellow palette for invalid input %s',
    invalid => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const palette = getMascotPalette(invalid as any);
      expect(palette).toBeDefined();
      expect(palette.bodyFill).toBe('#F7D145'); // yellow's bodyFill
    }
  );
});
