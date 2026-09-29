-- CreateEnum
CREATE TYPE "ClientReleaseBranch" AS ENUM ('release', 'test');

-- AlterTable
ALTER TABLE "ClientRelease" ADD COLUMN     "branch" "ClientReleaseBranch" NOT NULL DEFAULT 'release';

