//! Cron-driven trigger dispatch + manual run scheduling.
//!
//! F-7 fills this module: subscribe to the cron loop's ticks, look up
//! workflows whose `Trigger::Cron` expression matches, and call
//! `executor::dispatch_run`. Also adds `handle_run_now` for the
//! `workflows_run_now` RPC.
