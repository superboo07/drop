-- AlterTable
ALTER TABLE "GameVersion" ADD COLUMN     "baseVersionId" TEXT;

-- AddForeignKey
ALTER TABLE "GameVersion" ADD CONSTRAINT "GameVersion_baseVersionId_fkey" FOREIGN KEY ("baseVersionId") REFERENCES "GameVersion"("versionId") ON DELETE SET NULL ON UPDATE CASCADE;

-- Delta versions used to implicitly build on whichever version sat directly
-- below them in versionIndex order. Pin that down as an explicit base so
-- every existing chain resolves exactly as it did before.
UPDATE "GameVersion" AS v
SET "baseVersionId" = (
  SELECT p."versionId"
  FROM "GameVersion" AS p
  WHERE p."gameId" = v."gameId" AND p."versionIndex" < v."versionIndex"
  ORDER BY p."versionIndex" DESC
  LIMIT 1
)
WHERE v."delta" = true;
