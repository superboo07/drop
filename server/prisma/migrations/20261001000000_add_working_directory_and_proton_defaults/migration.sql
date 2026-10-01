-- AlterTable
ALTER TABLE "LaunchConfiguration" ADD COLUMN     "workingDirectory" TEXT;

-- AlterTable
ALTER TABLE "SetupConfiguration" ADD COLUMN     "workingDirectory" TEXT;

-- CreateTable
CREATE TABLE "ProtonDefaults" (
    "versionId" TEXT NOT NULL,
    "protonName" TEXT,
    "dxvk" BOOLEAN,
    "esync" BOOLEAN,
    "fsync" BOOLEAN,
    "locale" TEXT,
    "extraEnvVars" TEXT,
    "winetricks" TEXT[] DEFAULT ARRAY[]::TEXT[],

    CONSTRAINT "ProtonDefaults_pkey" PRIMARY KEY ("versionId")
);

-- AddForeignKey
ALTER TABLE "ProtonDefaults" ADD CONSTRAINT "ProtonDefaults_versionId_fkey" FOREIGN KEY ("versionId") REFERENCES "GameVersion"("versionId") ON DELETE CASCADE ON UPDATE CASCADE;
