# Local per-token discussion policy

This is a fixture-only policy core for the unclaimed per-token discussion lane in
`docs/TASKS.md`. It has no HTTP endpoint, database, wallet connection, chain
feed, or public comment UI. No comments are seeded or shown as live activity.

`createDiscussionService` requires three caller-provided boundaries:

- `authenticate(auth)` returns an opaque, stable `subject` from a real identity
  provider. The service never trusts a caller-supplied author ID.
- `authorizeModerator(auth)` grants review access. A post is hidden until a
  moderator approves it; a moderator can later hide it.
- `resolveToken(mint)` returns a matching, verified, finalized, active token
  record from an authenticated source. Missing or removed records make the
  thread unavailable, including previously approved comments. Resolver errors
  also fail closed.

Users opt in before posting and can opt out, which hides their posts. Accepted
posts are limited to three per identity per ten minutes; normalized duplicates
are rejected for 24 hours across token threads. Public listings expose no
author identity. A future UI must render comment text as text, never HTML, and
provide accessible report and moderation controls before enabling discussion.

All state is in memory. Restarting loses consent, moderation decisions and
rate history. Production work still needs durable identity, storage, abuse
reports, moderator audit trails, authenticated token indexing, reorg handling,
retention rules, and accessibility tests. This module does not rank tokens,
create engagement signals, promise rewards, or authorize any live service.

From the repository root, run:

```sh
node --test backend/discussion/service.test.mjs
```
