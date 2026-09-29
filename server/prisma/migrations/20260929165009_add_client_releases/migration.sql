-- CreateEnum
CREATE TYPE "ClientReleaseTarget" AS ENUM ('linux-appimage');

-- CreateEnum
CREATE TYPE "ClientReleaseArch" AS ENUM ('x86_64', 'aarch64');

-- CreateTable
CREATE TABLE "ClientRelease" (
    "id" TEXT NOT NULL,
    "tag" TEXT NOT NULL,
    "target" "ClientReleaseTarget" NOT NULL,
    "arch" "ClientReleaseArch" NOT NULL,
    "notes" TEXT NOT NULL DEFAULT '',
    "required" BOOLEAN NOT NULL DEFAULT false,
    "fileName" TEXT NOT NULL,
    "size" BIGINT NOT NULL,
    "sha256" TEXT NOT NULL,
    "uploadedAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    "publishedAt" TIMESTAMP(3),
    "withdrawnAt" TIMESTAMP(3),
    "uploaderId" TEXT,

    CONSTRAINT "ClientRelease_pkey" PRIMARY KEY ("id")
);

-- CreateIndex
CREATE INDEX "ClientRelease_target_arch_idx" ON "ClientRelease"("target", "arch");

-- CreateIndex
CREATE INDEX "ClientRelease_sha256_idx" ON "ClientRelease"("sha256");

-- AddForeignKey
ALTER TABLE "ClientRelease" ADD CONSTRAINT "ClientRelease_uploaderId_fkey" FOREIGN KEY ("uploaderId") REFERENCES "User"("id") ON DELETE SET NULL ON UPDATE CASCADE;
