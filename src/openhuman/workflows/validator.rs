//! Deterministic proposal validator (NFR-2.1.5: < 50 ms).
//!
//! F-11 fills this module: `validate(proposal, snapshot, phase) ->
//! Result<(), ProposalValidationError>` implementing every check from
//! FR-1.13.9 — required-fields, allowed node kinds for the current
//! phase, cron parse, edge integrity, required connections subset of
//! snapshot, allowed_connections walk. Fuzzy candidates for
//! `UnknownConnection` come from a Levenshtein helper here.
