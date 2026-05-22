//! System prompt builder for the `workflow_node` built-in agent (F-16).
//!
//! `workflow_node` is the constrained sub-agent the workflows executor
//! spawns for each `agent_prompt` node. The agent identity is
//! deliberately minimal — no orchestrator persona, no chat-tier
//! delegation tree, no profile/memory injection — because the user is
//! NOT on the other end of the conversation. The workflow runtime
//! is, and the contract with the runtime is simple: follow the
//! user-authored prompt verbatim, use only the constrained tool
//! allowlist, emit a terse summary, stop.
//!
//! See [`crate::openhuman::workflows::executor::run_agent_prompt`] —
//! it spawns this agent via
//! [`crate::openhuman::agent::Agent::from_config_for_agent_with_tool_override`]
//! and passes the per-run [`NodeAgentDefinition.allowed_tools`]
//! (baseline + connection-resolved + read-only workflow tools, per
//! ADR-016) as the override. The TOML's `[tools].named = []` is
//! REPLACED — not unioned — so this agent's effective tool surface
//! is exactly what F-8's `build_node_agent_definition` computed.

use crate::openhuman::context::prompt::{
    render_tools, render_user_files, render_workspace, PromptContext,
};
use anyhow::Result;

const ARCHETYPE: &str = include_str!("prompt.md");

pub fn build(ctx: &PromptContext<'_>) -> Result<String> {
    let mut out = String::with_capacity(4096);
    out.push_str(ARCHETYPE.trim_end());
    out.push_str("\n\n");

    // User files render iff the parent didn't opt out — for
    // workflow_node we leave the existing context::prompt machinery
    // alone (the omit_* flags already strip identity / memory / etc.).
    let user_files = render_user_files(ctx)?;
    if !user_files.trim().is_empty() {
        out.push_str(user_files.trim_end());
        out.push_str("\n\n");
    }

    let tools = render_tools(ctx)?;
    if !tools.trim().is_empty() {
        out.push_str(tools.trim_end());
        out.push_str("\n\n");
    }

    let workspace = render_workspace(ctx)?;
    if !workspace.trim().is_empty() {
        out.push_str(workspace.trim_end());
        out.push('\n');
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::context::prompt::{LearnedContextData, ToolCallFormat};
    use std::collections::HashSet;

    #[test]
    fn build_returns_workflow_node_archetype() {
        let visible: HashSet<String> = HashSet::new();
        let ctx = PromptContext {
            workspace_dir: std::path::Path::new("."),
            model_name: "test",
            agent_id: "workflow_node",
            tools: &[],
            skills: &[],
            dispatcher_instructions: "",
            learned: LearnedContextData::default(),
            visible_tool_names: &visible,
            tool_call_format: ToolCallFormat::PFormat,
            connected_integrations: &[],
            connected_identities_md: String::new(),
            include_profile: false,
            include_memory_md: false,
            curated_snapshot: None,
            user_identity: None,
        };
        let body = build(&ctx).unwrap();
        assert!(!body.is_empty());
        // Sanity-check the archetype copy made it into the rendered body
        // (catches a build-time misconfig where include_str! pointed at
        // a stale file).
        assert!(
            body.contains("Workflow Node"),
            "rendered body missing 'Workflow Node' header"
        );
        assert!(
            body.contains("Never delegate"),
            "rendered body missing the 'Never delegate' constraint"
        );
    }
}
