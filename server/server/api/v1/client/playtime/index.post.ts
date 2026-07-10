import { type } from "arktype";
import { ClientCapabilities } from "~/prisma/client/enums";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import { defineClientEventHandler } from "~/server/internal/clients/event-handler";
import prisma from "~/server/internal/db/database";

// Sessions are submitted as a batch (the client queues sessions while
// offline and drains the whole queue in one request) and id is
// client-generated - it's the idempotency key, see the transaction below.
const SubmitPlaytime = type({
  sessions: type({
    id: "string",
    gameId: "string",
    seconds: "number",
    startedAt: "string.date.iso.parse",
    endedAt: "string.date.iso.parse",
  }).array(),
}).configure(throwingArktype);

export default defineClientEventHandler(
  async (h3, { fetchClient, fetchUser }) => {
    const client = await fetchClient();
    if (!client.capabilities.includes(ClientCapabilities.TrackPlaytime))
      throw createError({
        statusCode: 403,
        statusMessage: "Capability not allowed.",
      });

    const user = await fetchUser();
    const { sessions } = await readDropValidatedBody(h3, SubmitPlaytime);

    if (sessions.length === 0) return { accepted: [] };

    // Sessions referencing a game this server doesn't have are silently
    // dropped (not included in `accepted`, so the client keeps them queued -
    // harmless, and self-correcting if the game later exists here).
    const gameIds = [...new Set(sessions.map((s) => s.gameId))];
    const games = await prisma.game.findMany({
      where: { id: { in: gameIds } },
      select: { id: true },
    });
    const validGameIds = new Set(games.map((g) => g.id));
    const validSessions = sessions.filter((s) => validGameIds.has(s.gameId));

    const accepted = await prisma.$transaction(async (tx) => {
      const ids = validSessions.map((s) => s.id);
      const existing = await tx.playtimeSession.findMany({
        where: { id: { in: ids } },
        select: { id: true },
      });
      const existingIds = new Set(existing.map((s) => s.id));
      const fresh = validSessions.filter((s) => !existingIds.has(s.id));

      if (fresh.length > 0) {
        // skipDuplicates guards a concurrent duplicate racing between the
        // findMany above and this insert.
        await tx.playtimeSession.createMany({
          data: fresh.map((s) => ({
            id: s.id,
            gameId: s.gameId,
            userId: user.id,
            clientId: client.id,
            seconds: s.seconds,
            startedAt: s.startedAt,
            endedAt: s.endedAt,
          })),
          skipDuplicates: true,
        });

        const secondsByGame = new Map<string, number>();
        for (const session of fresh) {
          secondsByGame.set(
            session.gameId,
            (secondsByGame.get(session.gameId) ?? 0) + session.seconds,
          );
        }
        for (const [gameId, seconds] of secondsByGame) {
          await tx.playtime.upsert({
            where: { gameId_userId: { gameId, userId: user.id } },
            create: { gameId, userId: user.id, seconds },
            update: { seconds: { increment: seconds } },
          });
        }
      }

      // Already-existing sessions are synced from the client's perspective
      // too (the server already has them accounted for), so they're reported
      // back the same as freshly-applied ones.
      return [...existingIds, ...fresh.map((s) => s.id)];
    });

    return { accepted };
  },
);
