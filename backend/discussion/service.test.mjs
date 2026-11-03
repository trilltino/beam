import assert from "node:assert/strict";
import test from "node:test";
import { createDiscussionService } from "./service.mjs";

const MINT = "So11111111111111111111111111111111111111112";
const OTHER_MINT = "11111111111111111111111111111111";

function fixture() {
  let clock = 1_000_000;
  const tokens = new Map([
    [MINT, { mint: MINT, verified: true, finalized: true, active: true }],
    [
      OTHER_MINT,
      { mint: OTHER_MINT, verified: true, finalized: true, active: true },
    ],
  ]);
  const service = createDiscussionService({
    authenticate: async (auth) =>
      auth === "alice" || auth === "bob" ? { subject: auth } : null,
    authorizeModerator: async (auth) => auth === "mod",
    resolveToken: async (mint) => tokens.get(mint),
    now: () => clock,
  });
  return { service, tokens, advance: (ms) => (clock += ms) };
}

test("requires authenticated opt-in and a finalized verified token", async () => {
  const { service, tokens } = fixture();
  await assert.rejects(service.optIn({ auth: "stranger" }), {
    code: "unauthorized",
  });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "Hello" }),
    { code: "opt_in_required" },
  );
  await service.optIn({ auth: "alice" });
  await assert.rejects(
    service.submit({ auth: "alice", mint: "invalid", text: "Hello" }),
    { code: "invalid_mint" },
  );
  tokens.set(MINT, {
    mint: MINT,
    verified: true,
    finalized: false,
    active: true,
  });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "Hello" }),
    { code: "token_unavailable" },
  );
  tokens.set(MINT, {
    mint: OTHER_MINT,
    verified: true,
    finalized: true,
    active: true,
  });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "Hello" }),
    { code: "token_unavailable" },
  );
});

test("moderation gates public comments and never exposes the author in listings", async () => {
  const { service } = fixture();
  await service.optIn({ auth: "alice" });
  const post = await service.submit({
    auth: "alice",
    mint: MINT,
    text: "  hello community  ",
  });
  assert.deepEqual(post, { id: "1", status: "pending" });
  assert.deepEqual(await service.list({ mint: MINT }), {
    state: "available",
    comments: [],
  });
  await assert.rejects(service.pending({ auth: "alice" }), {
    code: "forbidden",
  });
  assert.equal((await service.pending({ auth: "mod" }))[0].subject, "alice");
  await assert.rejects(
    service.moderate({ auth: "alice", id: post.id, decision: "approved" }),
    { code: "forbidden" },
  );
  await service.moderate({ auth: "mod", id: post.id, decision: "approved" });
  const visible = await service.list({ mint: MINT });
  assert.deepEqual(visible.comments, [
    { id: "1", mint: MINT, text: "hello community", createdAt: 1_000_000 },
  ]);
  assert.equal("subject" in visible.comments[0], false);
  await service.moderate({ auth: "mod", id: post.id, decision: "hidden" });
  assert.deepEqual((await service.list({ mint: MINT })).comments, []);
  await assert.rejects(
    service.moderate({ auth: "mod", id: post.id, decision: "approved" }),
    { code: "invalid_transition" },
  );
});

test("rate limit and normalized duplicate check span token threads and survive opt-out", async () => {
  const { service, advance } = fixture();
  await service.optIn({ auth: "alice" });
  await service.submit({ auth: "alice", mint: MINT, text: "First post" });
  await assert.rejects(
    service.submit({
      auth: "alice",
      mint: OTHER_MINT,
      text: "  FIRST   POST  ",
    }),
    { code: "duplicate" },
  );
  await service.submit({ auth: "alice", mint: MINT, text: "Second post" });
  await service.submit({ auth: "alice", mint: OTHER_MINT, text: "Third post" });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "Fourth post" }),
    { code: "rate_limited" },
  );
  await service.optOut({ auth: "alice" });
  await service.optIn({ auth: "alice" });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "Fourth post" }),
    { code: "rate_limited" },
  );
  advance(10 * 60 * 1000);
  await service.submit({ auth: "alice", mint: MINT, text: "Fourth post" });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "First post" }),
    { code: "duplicate" },
  );
});

test("opt-out and token removal hide previously approved content", async () => {
  const { service, tokens } = fixture();
  await service.optIn({ auth: "alice" });
  const post = await service.submit({
    auth: "alice",
    mint: MINT,
    text: "A local comment",
  });
  await service.moderate({ auth: "mod", id: post.id, decision: "approved" });
  tokens.delete(MINT);
  assert.deepEqual(await service.list({ mint: MINT }), {
    state: "unavailable",
    comments: [],
  });
  tokens.set(MINT, {
    mint: MINT,
    verified: true,
    finalized: true,
    active: true,
  });
  assert.equal((await service.list({ mint: MINT })).comments.length, 1);
  await service.optOut({ auth: "alice" });
  assert.deepEqual((await service.list({ mint: MINT })).comments, []);
  await service.optIn({ auth: "alice" });
  assert.deepEqual((await service.list({ mint: MINT })).comments, []);
});

test("opt-out during an asynchronous token check cannot leave a new pending post", async () => {
  let finishLookup;
  const service = createDiscussionService({
    authenticate: async () => ({ subject: "alice" }),
    authorizeModerator: async (auth) => auth === "mod",
    resolveToken: () => new Promise((resolve) => (finishLookup = resolve)),
  });
  await service.optIn({ auth: "alice" });
  const submission = service.submit({
    auth: "alice",
    mint: MINT,
    text: "Late post",
  });
  await new Promise((resolve) => setImmediate(resolve));
  await service.optOut({ auth: "alice" });
  finishLookup({ mint: MINT, verified: true, finalized: true, active: true });
  await assert.rejects(submission, { code: "opt_in_required" });
  assert.deepEqual(await service.pending({ auth: "mod" }), []);
});

test("token resolver outages fail closed for reads and new posts", async () => {
  const service = createDiscussionService({
    authenticate: async () => ({ subject: "alice" }),
    authorizeModerator: async () => false,
    resolveToken: async () => {
      throw new Error("indexer offline");
    },
  });
  await service.optIn({ auth: "alice" });
  assert.deepEqual(await service.list({ mint: MINT }), {
    state: "unavailable",
    comments: [],
  });
  await assert.rejects(
    service.submit({ auth: "alice", mint: MINT, text: "Hello" }),
    { code: "token_unavailable" },
  );
});

test("rejects malformed or oversized text and impossible moderation transitions", async () => {
  const { service } = fixture();
  await service.optIn({ auth: "bob" });
  for (const text of ["", "\u0001bad", "🟣".repeat(151)]) {
    await assert.rejects(service.submit({ auth: "bob", mint: MINT, text }), {
      code: "invalid_text",
    });
  }
  const post = await service.submit({
    auth: "bob",
    mint: MINT,
    text: "A good comment",
  });
  await service.moderate({ auth: "mod", id: post.id, decision: "rejected" });
  await assert.rejects(
    service.moderate({ auth: "mod", id: post.id, decision: "approved" }),
    { code: "invalid_transition" },
  );
  await assert.rejects(service.list({ mint: MINT, limit: 500 }), {
    code: "invalid_limit",
  });
});
