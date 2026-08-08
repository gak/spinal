# Coordinator Recovery Runbook

This runbook owns the detailed storage, process-cleanup, cancellation, and
crash-recovery mechanics for the optional coordinator capability described in
the [Spinal Application Consolidation plan](../PLAN-SPINAL-APPLICATION-CONSOLIDATION.md).
The plan owns the product boundary, phase order, safety outcomes, and decision
about whether a durable queue is warranted. This document does not authorize
mutation before both representative Phase 0 gates pass.

## Durability decision

Phase 3A begins as a guided, single-submission, one-shot flow. It still uses
immutable inputs, digest-bound Proposed artifacts, and safe restart from
Current, but it does not promise a resumable background queue. After several
production-like 3A runs, record whether real work is long-running, interrupted,
or concurrent enough to require resumable jobs.

- If not, retain the one-shot model and the minimum immutable audit records
  required for review and atomic promotion.
- If yes, implement the durable job state, reconciliation, and recovery model
  below as Phase 3B before promotion beta.

Do not build a queue merely because the prototype had one. Do not omit durable
correctness records merely because the queue is deferred.

## Process and locking boundary

- Hold one OS-level lock for a state root.
- Keep that coordinator-lifetime state-root ownership lock distinct from the
  per-call lock that serializes Spine CLI operations.
- Serialize Spine CLI operations unless licensing and concurrency are proven.
- Run long operations outside UI/request threads.
- Use per-job temporary directories, bounded output, a minimal environment,
  closed stdin, cancellation, timeout, and process-tree termination.
- Validate the approved Spine executable and exact 4.3.23 version before every
  job session.
- Treat animator submissions as trusted-team artifacts. Public untrusted upload
  processing is out of scope without isolated workers.

## Idempotence and mutation attempts

- Make every phase idempotent or explicitly non-repeatable.
- Never retry a new-animation editor mutation on the same Proposed copy.
  Discard uncertain output and rebuild from a freshly verified Current copy.
- A recovered workflow may repeat read-only analysis. Each candidate build gets
  a new attempt identity and fresh copy of Current.
- Cancellation is state-specific: read-only analysis may stop cleanly; a Spine
  process cancellation must finish cleanup or enter `cleanup uncertain`;
  cancellation is disabled once the atomic promotion commit begins.
- Mutation failures never offer a generic **Retry** action. They state whether
  Current changed, whether another attempt is safe, what evidence was retained,
  and exactly one state-specific next action.

## Cleanup uncertainty

Persist `cleanup uncertain` before releasing control after incomplete
process-tree termination. On restart, prove the recorded process and process
group are gone or require an explicit safe-recovery action before launching
Spine again. An in-memory poison flag is insufficient.

`cleanup uncertain` disables all further Spine work until the recorded process
state is verified and the explicit recovery action completes. Read-only viewing
of existing immutable artifacts may remain available if it cannot interact
with the uncertain workspace.

## Atomic proposal and promotion

1. Stage Proposed artifacts on the same filesystem as durable storage.
2. Validate and hash every artifact.
3. Flush files and containing directories.
4. Atomically rename the complete version into durable storage.
5. Use one database compare-and-swap transaction to advance Current only if its
   expected digest is still current.

Analysis, conflict choices, validation, per-animation acknowledgments, and
approval bind to exact Base, Current, Submission, and Proposed digests.
Rebuilding Proposed invalidates earlier acknowledgments. A stale Current never
advances the pointer.

Restoring an older immutable version is an audited forward operation. It
creates a new version/current decision and never mutates or erases history.

## Startup reconciliation

If Phase 3B is selected, reconcile every crash point at startup:

- incomplete temporary staging;
- a durable orphan with no database row;
- a committed database row without a current pointer;
- a current pointer without a complete immutable row;
- an interrupted cleanup or process group; and
- an interrupted schema migration or backup restore.

Orphan deletion is bounded and audited. Recovery never changes Current unless
the same atomic promotion preconditions are re-proven. An interrupted workflow
must be able to stop safely with Current unchanged.

Version the state schema. Before any migration, create and verify a restorable
backup; test both forward migration and backup restore before promotion beta.

## Privacy and user-facing outcomes

- Create state directories and files with private user-only permissions.
- Redact project paths, session capabilities, and secrets from user errors and
  ordinary logs.
- Retain enough evidence to explain an outcome without retaining license
  material or private assets in Git.
- Every failure message states: **Current changed?**, **safe to try again?**,
  and one next action.
