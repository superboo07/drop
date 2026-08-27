-- Repairs databases that applied 20260224145112_add_non_null_default_to_carousel_object_ids
-- before that migration was retroactively edited upstream to also create the
-- age-rating schema (the AgeRatingOrganization enum, GameAgeRating, its unique
-- index and its foreign key). Prisma tracks migrations by name, so on any
-- database that had already run the original version the edited SQL is silently
-- skipped forever -- leaving GameAgeRating missing, and making
-- 20260726041153_add_user_groups fail outright when it references the enum for
-- BannedAgeRating.
--
-- Dated to sort between add_playtime_session and add_user_groups so it runs
-- before the migration that needs the enum. Every statement is conditional, so
-- on a fresh database -- where 20260224145112 already created all of this -- it
-- is a no-op.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'AgeRatingOrganization') THEN
        CREATE TYPE "AgeRatingOrganization" AS ENUM ('ESRB', 'PEGI', 'CERO', 'USK', 'GRAC', 'ClassInd', 'ACB');
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS "GameAgeRating" (
    "id" TEXT NOT NULL,
    "organization" "AgeRatingOrganization" NOT NULL,
    "rating" TEXT NOT NULL,
    "ratingCoverUrl" TEXT,
    "gameId" TEXT NOT NULL,

    CONSTRAINT "GameAgeRating_pkey" PRIMARY KEY ("id")
);

CREATE UNIQUE INDEX IF NOT EXISTS "GameAgeRating_gameId_organization_key" ON "GameAgeRating"("gameId", "organization");

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'GameAgeRating_gameId_fkey') THEN
        ALTER TABLE "GameAgeRating" ADD CONSTRAINT "GameAgeRating_gameId_fkey" FOREIGN KEY ("gameId") REFERENCES "Game"("id") ON DELETE CASCADE ON UPDATE CASCADE;
    END IF;
END
$$;
