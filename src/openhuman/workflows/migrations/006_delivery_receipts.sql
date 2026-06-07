-- 006_delivery_receipts.sql — Trust UX T-1 receipts column on run steps.
--
-- Phase 2.5 (Trust UX) introduces `DeliveryReceipt`s — structured
-- records of real-world side effects a workflow run produced (email
-- sent, message posted, file created). They're emitted by
-- `composio_execute` on write-tool success and accumulated by the
-- workflow executor's F-16 subscriber alongside the existing
-- `ToolCallObservation`s.
--
-- Stored as a JSON-encoded `Vec<DeliveryReceipt>` on each
-- `workflow_run_steps` row. Default `'[]'` for forward-compat with
-- pre-T-1 rows + read-only / failed-before-write steps where the vec
-- is naturally empty.
--
-- T-2 reads this column via the unchanged `workflows_get_run` RPC and
-- renders each receipt as a plain-English row in the per-run outcome
-- card with a deep link to the resulting artifact ("📧 Sent email to
-- alediez2408@gmail.com  [Open in Gmail →]").

ALTER TABLE workflow_run_steps
  ADD COLUMN delivery_receipts_json TEXT NOT NULL DEFAULT '[]';
