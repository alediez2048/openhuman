//! F3-6 — safety preamble appended to the browser_action user prompt.
//!
//! Per the F3-6 ticket, the preamble codifies five behavioural rules
//! the LLM must respect when it has the browser_* tools in its
//! allowlist. Today this gets appended into the user-message prompt
//! by `executor::compose_browser_action_prompt`; F3-6 chunk 2 may
//! promote it into the workflow_node system prompt directly so it
//! benefits from the system-prompt prefix cache.

/// Static preamble text. Rendered verbatim — no templating.
const PREAMBLE: &str = "\n\n\
## Safety rules (you MUST follow these)\n\
\n\
1. **Never type credentials.** If you see a login page and you're not authenticated, \
   the user's session has expired. STOP and return a final response of \
   `{ \"status\": \"session_expired\", \"message\": \"User must re-authenticate before this workflow can run\" }`. \
   Do not attempt to type a username, password, 2FA code, recovery code, or any \
   other credential — there are no credentials available to you and you must not \
   guess.\n\
\n\
2. **Never solve CAPTCHAs.** If a CAPTCHA, reCAPTCHA, Cloudflare challenge, or any \
   other anti-bot check appears, STOP and return a final response of \
   `{ \"status\": \"captcha_blocked\", \"message\": \"<short description of the challenge>\" }`. \
   Do not try to click images, drag sliders, type prompts, or otherwise solve it.\n\
\n\
3. **Stay within allowed hosts.** Only navigate to URLs whose host is in the \
   allowed-hosts list above (the section labelled `Allowed hosts`). If your \
   goal requires reaching a host outside that list, STOP and return \
   `{ \"status\": \"host_not_allowed\", \"target\": \"<host>\" }`. \
   Don't follow redirects that cross host boundaries.\n\
\n\
4. **Single task per run.** You are running ONE workflow node toward ONE declared \
   goal. Don't browse adjacent content, don't open tangentially-useful pages, \
   don't \"explore\" the site. When the goal is complete, return your final \
   response and stop.\n\
\n\
5. **Observe before every act.** Always call `browser_observe` before `browser_act`. \
   The `element_id` you pass to `browser_act` must come from the most recent \
   snapshot — passing an id from a stale snapshot will fail with \
   `\"element [N] not in latest snapshot\"`.\n";

/// Return the safety preamble. Static string — no allocation in the
/// hot path.
pub fn safety_preamble() -> &'static str {
    PREAMBLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_covers_all_five_rules() {
        let p = safety_preamble();
        assert!(p.contains("Never type credentials"));
        assert!(p.contains("Never solve CAPTCHAs"));
        assert!(p.contains("Stay within allowed hosts"));
        assert!(p.contains("Single task per run"));
        assert!(p.contains("Observe before every act"));
    }

    #[test]
    fn preamble_includes_machine_readable_status_strings() {
        // The LLM is instructed to emit these exact status strings on
        // refusal; downstream consumers can pattern-match on them. Keep
        // this assertion tight so a future copy-edit doesn't silently
        // break the contract.
        let p = safety_preamble();
        assert!(p.contains("\"status\": \"session_expired\""));
        assert!(p.contains("\"status\": \"captcha_blocked\""));
        assert!(p.contains("\"status\": \"host_not_allowed\""));
    }
}
