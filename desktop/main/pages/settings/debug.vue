<template>
  <div class="divide-y divide-zinc-700">
    <div class="py-6">
      <h2 class="text-base font-semibold font-display leading-7 text-zinc-100">
        Debug Information
      </h2>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Technical information about your Drop client installation, helpful for
        debugging.
      </p>

      <div class="mt-10 space-y-8">
        <div>
          <div class="flex items-center gap-x-3">
            <FingerPrintIcon class="h-5 w-5 text-zinc-400" />
            <h3 class="text-sm font-medium leading-6 text-zinc-100">
              Client ID
            </h3>
          </div>
          <p class="mt-2 text-sm text-zinc-400 font-mono ml-8">
            {{ clientId || "Not signed in" }}
          </p>
        </div>

        <div>
          <div class="flex items-center gap-x-3">
            <TagIcon class="h-5 w-5 text-zinc-400" />
            <h3 class="text-sm font-medium leading-6 text-zinc-100">
              Version
            </h3>
          </div>
          <p class="mt-2 text-sm text-zinc-400 font-mono ml-8">
            {{ appVersion }}
          </p>
        </div>

        <div>
          <div class="flex items-center gap-x-3">
            <ComputerDesktopIcon class="h-5 w-5 text-zinc-400" />
            <h3 class="text-sm font-medium leading-6 text-zinc-100">
              Platform
            </h3>
          </div>
          <p class="mt-2 text-sm text-zinc-400 font-mono ml-8">
            {{ platformInfo }}
          </p>
        </div>

        <div>
          <div class="flex items-center gap-x-3">
            <ServerIcon class="h-5 w-5 text-zinc-400" />
            <h3 class="text-sm font-medium leading-6 text-zinc-100">
              Server URL
            </h3>
          </div>
          <p class="mt-2 text-sm text-zinc-400 font-mono ml-8">
            {{ baseUrl || "Not connected" }}
          </p>
        </div>

        <div>
          <div class="flex items-center gap-x-3">
            <FolderIcon class="h-5 w-5 text-zinc-400" />
            <h3 class="text-sm font-medium leading-6 text-zinc-100">
              Data Directory
            </h3>
          </div>
          <p class="mt-2 text-sm text-zinc-400 font-mono ml-8">
            {{ dataDir || "Unknown" }}
          </p>
        </div>

        <div class="pt-6 flex gap-x-4">
          <button
            @click="() => openDataDir()"
            type="button"
            class="inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
          >
            <FolderIcon class="h-5 w-5" aria-hidden="true" />
            Open Data Directory
          </button>
          <button
            @click="() => openLogFile()"
            type="button"
            class="inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
          >
            <DocumentTextIcon class="h-5 w-5" aria-hidden="true" />
            Open Log File
          </button>
          <button
            @click="() => sendLogs()"
            type="button"
            :disabled="sendState === 'sending' || !clientId || settings.forceOffline"
            class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3.5 py-2.5 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-zinc-700 hover:bg-zinc-700 disabled:opacity-50 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
          >
            <PaperAirplaneIcon class="h-5 w-5" aria-hidden="true" />
            {{ sendState === "sending" ? "Sending…" : "Send Logs to Server" }}
          </button>
        </div>
        <div class="flex max-w-3xl items-start justify-between gap-6">
          <div>
            <h4 class="text-sm font-medium text-zinc-100">
              Send logs automatically
            </h4>
            <p class="mt-1 text-sm text-zinc-400">
              When a game fails to launch or Drop crashes, send the logs to
              your server so its admins can see what went wrong.
            </p>
          </div>
          <Switch
            v-model="sendAutomatically"
            :class="[
              sendAutomatically ? 'bg-blue-600' : 'bg-zinc-700',
              'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
            ]"
          >
            <span class="sr-only">Send logs automatically</span>
            <span
              :class="[
                sendAutomatically ? 'translate-x-5' : 'translate-x-0',
                'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
              ]"
            />
          </Switch>
        </div>

        <p v-if="settings.forceOffline" class="text-sm text-zinc-400">
          Drop is in always-offline mode, so no logs are sent.
        </p>
        <p
          v-else-if="sendState === 'sent'"
          class="text-sm text-green-400"
        >
          Logs sent. Your server's admins can read them under Admin › Clients ›
          Logs.
        </p>
        <p v-else-if="sendError" class="text-sm text-red-400">
          Couldn't send logs: {{ sendError }}
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { platform, type } from "@tauri-apps/plugin-os";
import {
  FingerPrintIcon,
  TagIcon,
  ComputerDesktopIcon,
  ServerIcon,
  FolderIcon,
  CubeIcon,
  DocumentTextIcon,
  PaperAirplaneIcon,
} from "@heroicons/vue/24/outline";
import { open } from "@tauri-apps/plugin-shell";
import { Switch } from "@headlessui/vue";
import type { Settings } from "~/types";

const clientId = ref<string | null>(null);
// "<version>-g<commit>[.dirty]" for AppImage builds - the same string the
// AppImage's file name carries
const appVersion = ref<string>("");
invokeWithTimeout<{ appVersion: string }>("fetch_client_update_status").then(
  (status) => (appVersion.value = status.appVersion),
);
const platformInfo = ref("Loading...");
const baseUrl = ref<string | null>(null);
const dataDir = ref<string | null>(null);

const systemData = await invokeWithTimeout<{
  clientId: string;
  baseUrl: string;
  dataDir: string;
}>("fetch_system_data");

clientId.value = systemData.clientId;
baseUrl.value = systemData.baseUrl;
dataDir.value = systemData.dataDir;

const currentPlatform = await platform();
platformInfo.value = currentPlatform;

async function openDataDir() {
  if (!dataDir.value) return;
  try {
    await invokeWithTimeout("open_fs", { path: dataDir.value });
  } catch (error) {
    console.error("Failed to open data dir:", error);
  }
}

const settings = await invokeWithTimeout<Settings>("fetch_settings");
const sendAutomatically = ref(settings.sendLogsAutomatically);

watch(sendAutomatically, async (newValue) => {
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { sendLogsAutomatically: newValue },
    });
  } catch (error) {
    console.error("Failed to update send-logs setting:", error);
    sendAutomatically.value = !newValue;
  }
});

const sendState = ref<"idle" | "sending" | "sent">("idle");
const sendError = ref<string>();

// Uploads drop.log, the previous run's log and the last game's logs
async function sendLogs() {
  sendState.value = "sending";
  sendError.value = undefined;
  try {
    await invokeWithTimeout("send_client_logs", {}, 150_000);
    sendState.value = "sent";
  } catch (error) {
    sendError.value = `${error}`;
    sendState.value = "idle";
  }
}

async function openLogFile() {
  if (!dataDir.value) return;
  try {
    const logPath = `${dataDir.value}/drop.log`;
    await invokeWithTimeout("open_fs", { path: logPath });
  } catch (error) {
    console.error("Failed to open log file:", error);
  }
}
</script>
