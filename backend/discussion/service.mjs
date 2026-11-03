const MINT_PATTERN = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/;
const MAX_TEXT_BYTES = 600;
const RATE_WINDOW_MS = 10 * 60 * 1000;
const DUPLICATE_WINDOW_MS = 24 * 60 * 60 * 1000;
const RATE_LIMIT = 3;

export class DiscussionError extends Error {
  constructor(code) {
    super(code);
    this.name = "DiscussionError";
    this.code = code;
  }
}

function requireMint(mint) {
  if (typeof mint !== "string" || !MINT_PATTERN.test(mint)) {
    throw new DiscussionError("invalid_mint");
  }
}

function requireText(text) {
  if (typeof text !== "string") throw new DiscussionError("invalid_text");
  const trimmed = text.trim();
  if (
    !trimmed ||
    Buffer.byteLength(trimmed, "utf8") > MAX_TEXT_BYTES ||
    /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/u.test(trimmed)
  ) {
    throw new DiscussionError("invalid_text");
  }
  return trimmed;
}

function publicComment(post) {
  return {
    id: post.id,
    mint: post.mint,
    text: post.text,
    createdAt: post.createdAt,
  };
}

// Local policy core. The caller must supply real authentication and a trusted,
// finalized token resolver before exposing any endpoint or public UI.
export function createDiscussionService({
  authenticate,
  authorizeModerator,
  resolveToken,
  now = Date.now,
}) {
  if (
    [authenticate, authorizeModerator, resolveToken, now].some(
      (value) => typeof value !== "function",
    )
  ) {
    throw new TypeError(
      "discussion service requires authentication, moderation, token, and clock adapters",
    );
  }

  const optedIn = new Set();
  const posts = new Map();
  const attempts = new Map();
  let nextId = 1;

  async function subject(auth) {
    const identity = await authenticate(auth);
    if (typeof identity?.subject !== "string" || !identity.subject.trim()) {
      throw new DiscussionError("unauthorized");
    }
    return identity.subject.trim();
  }

  async function moderator(auth) {
    if ((await authorizeModerator(auth)) !== true)
      throw new DiscussionError("forbidden");
  }

  async function tokenAvailable(mint) {
    try {
      const token = await resolveToken(mint);
      return (
        token?.mint === mint &&
        token.verified === true &&
        token.finalized === true &&
        token.active === true
      );
    } catch {
      return false;
    }
  }

  function timestamp() {
    const value = now();
    if (!Number.isSafeInteger(value) || value < 0)
      throw new DiscussionError("clock_unavailable");
    return value;
  }

  return {
    async optIn({ auth }) {
      optedIn.add(await subject(auth));
      return { optedIn: true };
    },

    async optOut({ auth }) {
      const actor = await subject(auth);
      optedIn.delete(actor);
      for (const post of posts.values()) {
        if (post.subject === actor) post.status = "hidden";
      }
      return { optedIn: false };
    },

    async submit({ auth, mint, text }) {
      const actor = await subject(auth);
      if (!optedIn.has(actor)) throw new DiscussionError("opt_in_required");
      requireMint(mint);
      if (!(await tokenAvailable(mint)))
        throw new DiscussionError("token_unavailable");
      if (!optedIn.has(actor)) throw new DiscussionError("opt_in_required");
      const cleanText = requireText(text);
      const at = timestamp();
      const fingerprint = cleanText
        .normalize("NFKC")
        .toLowerCase()
        .replace(/\s+/gu, " ");
      const recent = (attempts.get(actor) ?? []).filter(
        (entry) => entry.at <= at && at - entry.at < DUPLICATE_WINDOW_MS,
      );
      if (recent.some((entry) => entry.fingerprint === fingerprint))
        throw new DiscussionError("duplicate");
      if (
        recent.filter((entry) => at - entry.at < RATE_WINDOW_MS).length >=
        RATE_LIMIT
      ) {
        throw new DiscussionError("rate_limited");
      }

      const post = {
        id: String(nextId++),
        subject: actor,
        mint,
        text: cleanText,
        createdAt: at,
        status: "pending",
      };
      posts.set(post.id, post);
      recent.push({ at, fingerprint });
      attempts.set(actor, recent);
      return { id: post.id, status: post.status };
    },

    async pending({ auth }) {
      await moderator(auth);
      return [...posts.values()]
        .filter((post) => post.status === "pending")
        .map((post) => ({ ...publicComment(post), subject: post.subject }));
    },

    async moderate({ auth, id, decision }) {
      await moderator(auth);
      const post = posts.get(id);
      if (
        !post ||
        !(
          (post.status === "pending" &&
            ["approved", "rejected"].includes(decision)) ||
          (post.status === "approved" && decision === "hidden")
        )
      ) {
        throw new DiscussionError("invalid_transition");
      }
      post.status = decision;
      return { id: post.id, status: post.status };
    },

    async list({ mint, limit = 20 }) {
      requireMint(mint);
      if (!Number.isInteger(limit) || limit < 1 || limit > 50)
        throw new DiscussionError("invalid_limit");
      if (!(await tokenAvailable(mint)))
        return { state: "unavailable", comments: [] };
      return {
        state: "available",
        comments: [...posts.values()]
          .filter((post) => post.mint === mint && post.status === "approved")
          .sort(
            (a, b) => b.createdAt - a.createdAt || Number(b.id) - Number(a.id),
          )
          .slice(0, limit)
          .map(publicComment),
      };
    },
  };
}
