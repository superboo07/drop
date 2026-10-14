-- CreateEnum
CREATE TYPE "ClientLogTrigger" AS ENUM ('manual', 'crash', 'launch');

-- AlterEnum
ALTER TYPE "ClientCapabilities" ADD VALUE 'clientLogs';

-- AlterTable
ALTER TABLE "ApplicationSettings" ADD COLUMN     "clientLogsAutoSend" BOOLEAN NOT NULL DEFAULT true,
ADD COLUMN     "clientLogsMaxSizeMB" INTEGER NOT NULL DEFAULT 20,
ADD COLUMN     "clientLogsPerClient" INTEGER NOT NULL DEFAULT 20,
ADD COLUMN     "clientLogsRetentionDays" INTEGER NOT NULL DEFAULT 30;

-- CreateTable
CREATE TABLE "ClientLogBundle" (
    "id" TEXT NOT NULL,
    "clientId" TEXT,
    "clientName" TEXT NOT NULL,
    "userName" TEXT NOT NULL,
    "trigger" "ClientLogTrigger" NOT NULL,
    "version" TEXT NOT NULL,
    "arch" TEXT NOT NULL,
    "session" TEXT NOT NULL,
    "gameId" TEXT,
    "gameName" TEXT,
    "summary" JSONB,
    "environment" JSONB NOT NULL DEFAULT '[]',
    "files" JSONB NOT NULL DEFAULT '[]',
    "errors" INTEGER NOT NULL DEFAULT 0,
    "warnings" INTEGER NOT NULL DEFAULT 0,
    "size" INTEGER NOT NULL DEFAULT 0,
    "receivedAt" TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT "ClientLogBundle_pkey" PRIMARY KEY ("id")
);

-- CreateIndex
CREATE INDEX "ClientLogBundle_receivedAt_idx" ON "ClientLogBundle"("receivedAt");

-- CreateIndex
CREATE INDEX "ClientLogBundle_clientId_idx" ON "ClientLogBundle"("clientId");

-- AddForeignKey
ALTER TABLE "ClientLogBundle" ADD CONSTRAINT "ClientLogBundle_clientId_fkey" FOREIGN KEY ("clientId") REFERENCES "Client"("id") ON DELETE SET NULL ON UPDATE CASCADE;

