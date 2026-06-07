//! T-3 (Phase 2.5 Trust UX) — pre-flight validation pipeline that runs
//! when the user clicks **Save & Enable** on a workflow proposal.
//!
//! Probes every component the workflow will need at runtime (model,
//! connections) and returns a [`PreflightReport`]. The UI gates Save &
//! Enable on `passed = true`; failed checks render their `fix_hint` as
//! a clear next action so the user can fix the underlying issue
//! without trial-and-error.
//!
//! ## Scope
//!
//! Phase 2.5 T-3 ships two of the four check kinds from the original
//! ticket:
//!
//! - [`PreflightCheckKind::ModelAvailable`] — resolves the proposal's
//!   `model_tier` (or the workflow_node default) against the OpenHuman
//!   backend's canonical tier list. Catches `claude-opus-4-7`-style
//!   stale slugs at Save time instead of at first run.
//! - [`PreflightCheckKind::ConnectionLive`] — for each `ConnectionRef`
//!   in `allowed_connections`, asks the connections aggregator whether
//!   the connection exists and is in the `Connected` state.
//!
//! Deferred to T-3b:
//! - `ConnectionAuthProbe` — costs $0.005/save, needs a curated probe
//!   slug per toolkit. Real upstream auth failures still surface
//!   honestly via F-16's tool-failure path at run time.
//! - `ToolSlugResolvable` — best-effort prompt parsing + a per-toolkit
//!   tool catalog round-trip. The drafter's existing validation already
//!   rejects malformed slugs at proposal time.

use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use crate::openhuman::workflows::types::{NodeConfig, WorkflowProposal};

use serde::{Deserialize, Serialize};

/// Result of running the pre-flight pipeline against a workflow
/// proposal. The UI inspects `passed` to gate the Save & Enable button
/// and renders `checks` so the user sees every probe result at once.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightReport {
    /// `true` iff every check returned [`PreflightStatus::Pass`] or
    /// [`PreflightStatus::Warn`]. Any [`PreflightStatus::Fail`] flips
    /// this to `false`. The UI uses this single bit to enable /
    /// disable the [Save & Enable] button without re-reasoning about
    /// every check.
    pub passed: bool,
    /// Every check the pipeline ran, in the order it ran them. Warns
    /// and passes are kept so the UI can render a complete picture.
    pub checks: Vec<PreflightCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightCheck {
    pub kind: PreflightCheckKind,
    pub status: PreflightStatus,
    /// Short human-readable description of what was checked + the
    /// result. Surfaced verbatim in the UI.
    pub detail: String,
    /// Optional fix-it hint. When present, the UI renders it next to
    /// the failed check so the user knows exactly what to change.
    /// `None` for passes (no fix needed) and for some warns.
    pub fix_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PreflightCheckKind {
    /// Resolved the proposal's effective model against the OpenHuman
    /// backend's canonical tier list (`reasoning-v1`, `chat-v1`,
    /// `agentic-v1`, `coding-v1`, `reasoning-quick-v1`,
    /// `summarization-v1`). Unknown tiers fail with a fix hint
    /// pointing at the canonical list.
    ModelAvailable { tier: String },
    /// Verified the specific `ConnectionRef` is present in the
    /// aggregator snapshot and in the `Connected` state. Fails when
    /// the connection isn't set up; passes when it is.
    ConnectionLive { connection: ConnectionRef },
    /// The connections aggregator couldn't be reached during the
    /// pre-flight run. Emitted as `Warn` (not `Fail`) so a transient
    /// backend hiccup doesn't block the user from saving a workflow —
    /// any real connection issue will still fail honestly at first run
    /// via F-16's tool-failure path.
    AggregatorUnreachable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreflightStatus {
    /// Check ran cleanly. No issue, no fix needed.
    Pass,
    /// Check produced a non-blocking warning (e.g. transient backend
    /// failure that means the check couldn't run, but isn't itself
    /// evidence of a problem). The UI surfaces the warn but doesn't
    /// gate Save.
    Warn,
    /// Check found a definite problem. The UI blocks Save & Enable
    /// and surfaces the fix hint.
    Fail,
}

impl PreflightStatus {
    fn blocks_save(self) -> bool {
        matches!(self, Self::Fail)
    }
}

/// Run the pre-flight pipeline against the proposal. Async because the
/// connections check reads through to the per-mechanism stores.
///
/// Returns a [`PreflightReport`] with one [`PreflightCheck`] per probe.
/// Never errors at the top level — backend hiccups during probing
/// surface as `Warn`-level checks so the user can still save.
pub async fn run_preflight(config: &Config, proposal: &WorkflowProposal) -> PreflightReport {
    let mut checks = Vec::new();

    // Check 1: model availability. Each agent_prompt node carries its
    // own optional model_tier override; check every node's effective
    // tier separately so a 5-node chain with one bad tier surfaces
    // the specific failing node.
    for node in &proposal.nodes {
        if let NodeConfig::AgentPrompt(cfg) = &node.config {
            checks.push(check_model_available(cfg.model_tier.as_deref()));
        }
    }

    // Check 2: connection liveness. Aggregator snapshot is read once
    // and reused for every ConnectionRef. Any aggregator failure
    // surfaces as a single Warn — without the snapshot we can't run
    // ConnectionLive checks, but we also don't want to block Save on
    // a transient backend hiccup.
    let referenced = collect_referenced_connections(proposal);
    if !referenced.is_empty() {
        match aggregator::list_all(config).await {
            Ok(views) => {
                let snapshot = ConnectionsSnapshot::new(views);
                for connection in referenced {
                    checks.push(check_connection_live(connection, &snapshot));
                }
            }
            Err(err) => {
                checks.push(PreflightCheck {
                    kind: PreflightCheckKind::AggregatorUnreachable,
                    status: PreflightStatus::Warn,
                    detail: format!(
                        "Couldn't reach the connections aggregator to verify connections: {err:#}"
                    ),
                    fix_hint: Some(
                        "The workflow saved with this warning will fail at first run if a connection is missing — try again to confirm everything is wired up."
                            .into(),
                    ),
                });
            }
        }
    }

    let passed = !checks.iter().any(|c| c.status.blocks_save());
    PreflightReport { passed, checks }
}

// ── check implementations ──────────────────────────────────────────

/// Validate that the configured `model_tier` is a canonical OpenHuman
/// backend tier. `None` means "use the workflow_node default" which is
/// always valid — only an explicit non-canonical value fails.
///
/// The valid tier list is the same one
/// [`crate::openhuman::inference::provider::factory::is_known_openhuman_tier`]
/// recognises; copied here as a stable string slice so the UI can
/// surface it as a fix hint without an extra round-trip into the
/// inference layer.
fn check_model_available(tier: Option<&str>) -> PreflightCheck {
    let Some(tier) = tier.map(str::trim).filter(|s| !s.is_empty()) else {
        return PreflightCheck {
            kind: PreflightCheckKind::ModelAvailable {
                tier: "(workflow_node default)".to_string(),
            },
            status: PreflightStatus::Pass,
            detail: "No model override — using the workflow_node default (agentic-v1).".to_string(),
            fix_hint: None,
        };
    };
    if crate::openhuman::inference::provider::factory::is_known_openhuman_tier(tier) {
        return PreflightCheck {
            kind: PreflightCheckKind::ModelAvailable {
                tier: tier.to_string(),
            },
            status: PreflightStatus::Pass,
            detail: format!("Model `{tier}` is a recognized OpenHuman backend tier."),
            fix_hint: None,
        };
    }
    PreflightCheck {
        kind: PreflightCheckKind::ModelAvailable {
            tier: tier.to_string(),
        },
        status: PreflightStatus::Fail,
        detail: format!(
            "Model `{tier}` isn't in the OpenHuman backend's tier list. Saving this workflow would cause the first run to fail with `Model '{tier}' is not available`."
        ),
        fix_hint: Some(
            "Replace with one of: reasoning-v1, chat-v1, agentic-v1, coding-v1, reasoning-quick-v1, summarization-v1. Leave blank to use the workflow_node default."
                .into(),
        ),
    }
}

fn check_connection_live(
    connection: ConnectionRef,
    snapshot: &ConnectionsSnapshot,
) -> PreflightCheck {
    let live = snapshot.is_connected(&connection);
    let label = describe_connection(&connection);
    if live {
        PreflightCheck {
            kind: PreflightCheckKind::ConnectionLive { connection },
            status: PreflightStatus::Pass,
            detail: format!("{label} is connected and live."),
            fix_hint: None,
        }
    } else {
        PreflightCheck {
            kind: PreflightCheckKind::ConnectionLive {
                connection: connection.clone(),
            },
            status: PreflightStatus::Fail,
            detail: format!("{label} is not connected. The workflow can't reach this service."),
            fix_hint: Some(format!(
                "Open Settings → Connections and connect {label} before enabling this workflow."
            )),
        }
    }
}

// ── helpers ────────────────────────────────────────────────────────

/// Collect every `ConnectionRef` mentioned by an `agent_prompt` node's
/// `allowed_connections`. Phase 1 + 2 workflows only carry refs on
/// agent_prompt nodes; other node kinds (`tool_call`, `http_request`,
/// `channel_message`, `condition`, `delay`) don't have an
/// `allowed_connections` field.
fn collect_referenced_connections(proposal: &WorkflowProposal) -> Vec<ConnectionRef> {
    let mut out = Vec::new();
    for node in &proposal.nodes {
        if let NodeConfig::AgentPrompt(cfg) = &node.config {
            for r in &cfg.allowed_connections {
                if !out.contains(r) {
                    out.push(r.clone());
                }
            }
        }
    }
    out
}

/// Render a `ConnectionRef` as a short human-readable label for use
/// in `detail` and `fix_hint` strings.
fn describe_connection(r#ref: &ConnectionRef) -> String {
    match r#ref {
        ConnectionRef::Composio {
            toolkit_id,
            account_id: _,
        } => format!("Composio: {toolkit_id}"),
        ConnectionRef::Channel {
            provider,
            channel_id: _,
        } => format!("Channel: {provider}"),
        ConnectionRef::Webview {
            provider,
            account_id: _,
        } => format!("Webview: {provider}"),
        ConnectionRef::Builtin { integration } => format!("Built-in: {integration}"),
        ConnectionRef::Mcp { server_id, .. } => format!("MCP: {server_id}"),
        ConnectionRef::GenericHttp { connection_id } => format!("HTTP: {connection_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::connections::types::ConnectionRef;
    use crate::openhuman::workflows::types::{
        AgentPromptConfig, Confidence, Node, NodeKind, OnErrorPolicy, Trigger, WorkflowProposal,
        WorkflowSettings,
    };

    fn proposal_with(nodes: Vec<Node>, required: Vec<ConnectionRef>) -> WorkflowProposal {
        WorkflowProposal {
            name: "T-3 test".into(),
            description: "".into(),
            trigger: Trigger::Manual,
            nodes,
            edges: vec![],
            settings: WorkflowSettings {
                timeout_secs: 300,
                on_error: OnErrorPolicy::Halt,
            },
            required_connections: required,
            rationale: vec![],
            confidence: Confidence::High,
        }
    }

    fn agent_prompt_node(model_tier: Option<&str>, allowed: Vec<ConnectionRef>) -> Node {
        Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "do a thing".into(),
                allowed_connections: allowed,
                iteration_cap: 12,
                model_tier: model_tier.map(str::to_string),
            }),
            position: None,
            retry_policy: None,
        }
    }

    // ── ModelAvailable ──

    #[test]
    fn check_model_passes_for_canonical_tier() {
        let check = check_model_available(Some("agentic-v1"));
        assert_eq!(check.status, PreflightStatus::Pass);
        assert!(check.fix_hint.is_none());
    }

    #[test]
    fn check_model_passes_for_none_tier_using_default() {
        let check = check_model_available(None);
        assert_eq!(check.status, PreflightStatus::Pass);
    }

    #[test]
    fn check_model_passes_for_blank_string() {
        // Blank string after trim is treated same as None.
        let check = check_model_available(Some("  "));
        assert_eq!(check.status, PreflightStatus::Pass);
    }

    #[test]
    fn check_model_fails_for_anthropic_direct_slug() {
        // The exact failure mode I shipped + immediately broke on
        // 2026-06-07 — `claude-opus-4-7` is Anthropic's slug, not an
        // OpenHuman backend tier. T-3 catches this at Save time.
        let check = check_model_available(Some("claude-opus-4-7"));
        assert_eq!(check.status, PreflightStatus::Fail);
        let hint = check.fix_hint.expect("fail must carry a fix hint");
        assert!(
            hint.contains("agentic-v1"),
            "fix hint must point at canonical tiers: {hint}"
        );
        assert!(check.detail.contains("claude-opus-4-7"));
    }

    #[test]
    fn check_model_passes_for_hint_form() {
        // `hint:agentic` is the proper string form. Must pass.
        let check = check_model_available(Some("hint:agentic"));
        assert_eq!(check.status, PreflightStatus::Pass);
    }

    // ── ConnectionLive ──

    #[test]
    fn check_connection_fails_when_snapshot_empty() {
        let check = check_connection_live(
            ConnectionRef::Composio {
                toolkit_id: "gmail".into(),
                account_id: None,
            },
            &ConnectionsSnapshot::empty(),
        );
        assert_eq!(check.status, PreflightStatus::Fail);
        let hint = check.fix_hint.unwrap();
        assert!(hint.contains("Settings → Connections"));
        assert!(hint.contains("gmail"));
    }

    // ── full pipeline ──

    #[tokio::test]
    async fn run_preflight_passes_workflow_with_default_model_and_no_connections() {
        let config = crate::openhuman::config::Config::default();
        let proposal = proposal_with(vec![agent_prompt_node(None, vec![])], vec![]);
        let report = run_preflight(&config, &proposal).await;
        assert!(report.passed, "empty connections + default model must pass");
        // One check: the ModelAvailable for the single agent_prompt node.
        assert_eq!(report.checks.len(), 1);
    }

    #[tokio::test]
    async fn run_preflight_fails_workflow_with_bad_model_tier() {
        let config = crate::openhuman::config::Config::default();
        let proposal = proposal_with(
            vec![agent_prompt_node(Some("claude-opus-4-7"), vec![])],
            vec![],
        );
        let report = run_preflight(&config, &proposal).await;
        assert!(
            !report.passed,
            "claude-opus-4-7 model_tier must fail preflight (would 400 at first run)"
        );
        assert!(report.checks.iter().any(|c| c.status == PreflightStatus::Fail
            && matches!(
                c.kind,
                PreflightCheckKind::ModelAvailable { ref tier } if tier == "claude-opus-4-7"
            )));
    }

    #[tokio::test]
    async fn run_preflight_fails_workflow_referencing_missing_connection() {
        // Default Config + tempdir workspace — aggregator will return
        // empty connections, so any referenced connection fails the
        // ConnectionLive check.
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::openhuman::config::Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        let proposal = proposal_with(
            vec![agent_prompt_node(
                None,
                vec![ConnectionRef::Composio {
                    toolkit_id: "gmail".into(),
                    account_id: None,
                }],
            )],
            vec![ConnectionRef::Composio {
                toolkit_id: "gmail".into(),
                account_id: None,
            }],
        );
        let report = run_preflight(&config, &proposal).await;
        assert!(
            !report.passed,
            "missing connection must fail preflight"
        );
        let fail = report
            .checks
            .iter()
            .find(|c| matches!(c.kind, PreflightCheckKind::ConnectionLive { .. }))
            .expect("connection check must run");
        assert_eq!(fail.status, PreflightStatus::Fail);
    }

    #[tokio::test]
    async fn run_preflight_dedupes_same_connection_across_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::openhuman::config::Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        let conn = ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        };
        let proposal = proposal_with(
            vec![
                agent_prompt_node(None, vec![conn.clone()]),
                Node {
                    id: "n2".into(),
                    kind: NodeKind::AgentPrompt,
                    config: NodeConfig::AgentPrompt(AgentPromptConfig {
                        prompt: "second".into(),
                        allowed_connections: vec![conn.clone()],
                        iteration_cap: 12,
                        model_tier: None,
                    }),
                    position: None,
                    retry_policy: None,
                },
            ],
            vec![conn.clone()],
        );
        let report = run_preflight(&config, &proposal).await;
        // Two model checks (one per node), but only ONE connection check
        // because the connection is deduplicated.
        let connection_checks = report
            .checks
            .iter()
            .filter(|c| matches!(c.kind, PreflightCheckKind::ConnectionLive { .. }))
            .count();
        assert_eq!(
            connection_checks, 1,
            "same ConnectionRef across nodes must produce one check, not N"
        );
    }
}
