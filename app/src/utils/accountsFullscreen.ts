/** Sentinel id for the always-present agent entry in the Accounts page. */
export const AGENT_ACCOUNT_ID = '__agent__';

/**
 * Always returns `false` — the auto-hide-the-tab-bar fullscreen mode was
 * disabled after live testing showed it strands the user inside an
 * embedded provider webview with no visible way back. The CEF child
 * webview composites natively above the HTML layer, so the prior
 * 12px hover-strip reveal affordance was hidden behind the webview and
 * unreachable; users reported "the connector connects but there's no
 * easy exit" once Instagram / LinkedIn / etc. went fullscreen.
 *
 * Tab bar now stays mounted on every route. The kept signature lets
 * callers (BottomTabBar, AppShell) opt in again later if a real
 * fullscreen experience ships (with an HTML reveal handle positioned
 * outside the webview rect).
 */
export function isAccountsFullscreen(
  _pathname: string,
  _activeAccountId: string | null | undefined
): boolean {
  return false;
}
