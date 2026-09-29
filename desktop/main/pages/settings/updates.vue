<template>
  <div class="border-b border-zinc-700 py-5">
    <h3 class="text-base font-semibold font-display leading-6 text-zinc-100">
      Updates
    </h3>
  </div>

  <!-- can't update this copy -->
  <div
    v-if="status?.unsupportedReason"
    class="mt-6 flex items-start gap-4 rounded-xl bg-zinc-800/50 p-5 ring-1 ring-zinc-700/60"
  >
    <span
      class="grid size-10 shrink-0 place-items-center rounded-full bg-zinc-700/60 text-zinc-400"
    >
      <ExclamationTriangleIcon class="size-6" />
    </span>
    <div>
      <div class="text-base font-semibold text-zinc-100">
        Drop can't update itself here
      </div>
      <p class="mt-1 text-sm text-zinc-400">{{ status.unsupportedReason }}</p>
      <p class="mt-2 text-xs text-zinc-500">
        Drop {{ status.appVersion }} · {{ status.arch }}
      </p>
    </div>
  </div>

  <template v-else-if="status">
    <!-- status card -->
    <div
      class="mt-6 flex flex-wrap items-start justify-between gap-6 rounded-xl bg-zinc-800/50 p-5 ring-1 ring-zinc-700/60"
    >
      <div class="flex min-w-0 gap-4">
        <span
          :class="[
            card.iconClass,
            'grid size-10 shrink-0 place-items-center rounded-full',
          ]"
        >
          <component :is="card.icon" class="size-6" />
        </span>
        <div class="min-w-0">
          <div class="text-base font-semibold text-zinc-100">
            {{ card.title }}
          </div>
          <div class="mt-0.5 text-sm text-zinc-400">
            Drop {{ status.current?.tag ?? status.appVersion }} ·
            {{ status.branch === "test" ? "Test" : "Release" }} branch ·
            AppImage {{ status.arch }}
          </div>
          <div class="mt-2 text-xs text-zinc-500 tabular-nums">
            <template v-if="status.lastChecked">
              Last checked {{ formatTime(status.lastChecked) }}
            </template>
            <template v-else>Not checked yet</template>
            <template v-if="status.current">
              · Release ID
              <span class="font-mono" :title="status.current.id"
                >{{ status.current.id.slice(0, 8) }}…</span
              >
            </template>
          </div>
          <p v-if="status.error" class="mt-2 text-sm text-red-400">
            {{ status.error }}
          </p>
        </div>
      </div>
      <div class="flex shrink-0 gap-2">
        <button
          v-if="status.update || status.installed"
          type="button"
          class="rounded-md bg-blue-600 px-3 py-2 text-sm font-semibold text-white hover:bg-blue-500"
          @click="dialog = true"
        >
          {{ status.installed ? "Restart" : "See what's new" }}
        </button>
        <button
          type="button"
          :disabled="status.checking"
          class="inline-flex items-center gap-2 rounded-md bg-zinc-700 px-3 py-2 text-sm font-semibold text-zinc-100 hover:bg-zinc-600 disabled:opacity-60"
          @click="check"
        >
          <ArrowPathIcon
            :class="['size-4', status.checking ? 'animate-spin' : '']"
          />
          {{ status.checking ? "Checking…" : "Check now" }}
        </button>
      </div>
    </div>

    <!-- branch -->
    <div class="mt-8">
      <h4 class="text-sm font-semibold text-zinc-100">Update branch</h4>
      <p class="mt-1 max-w-xl text-sm text-zinc-400">
        Choose which builds this computer is offered.
      </p>
      <RadioGroup
        :model-value="pendingBranch ?? branch"
        class="mt-4 grid max-w-3xl gap-3 sm:grid-cols-2"
        @update:model-value="chooseBranch"
      >
        <RadioGroupOption
          v-for="option in branchOptions"
          :key="option.value"
          v-slot="{ checked }"
          :value="option.value"
          as="template"
        >
          <div
            :class="[
              checked
                ? 'bg-zinc-800/60 ring-2 ring-blue-500'
                : 'bg-zinc-800/30 ring-1 ring-zinc-700 hover:bg-zinc-800/50',
              'flex cursor-pointer gap-3 rounded-lg p-4 focus:outline-none',
            ]"
          >
            <span
              :class="[
                checked
                  ? 'border-4 border-blue-500 bg-white'
                  : 'border border-zinc-500',
                'mt-0.5 size-4 shrink-0 rounded-full',
              ]"
            />
            <span>
              <RadioGroupLabel
                as="span"
                class="block text-sm font-medium text-zinc-100"
                >{{ option.label }}</RadioGroupLabel
              >
              <RadioGroupDescription
                as="span"
                class="mt-1 block text-xs leading-5 text-zinc-400"
                >{{ option.description }}</RadioGroupDescription
              >
            </span>
          </div>
        </RadioGroupOption>
      </RadioGroup>

      <div
        v-if="pendingBranch"
        class="mt-3 max-w-3xl rounded-md bg-yellow-500/10 px-4 py-3 text-sm text-yellow-100 ring-1 ring-yellow-500/20"
      >
        <p>
          You're on test build
          <span class="font-mono text-xs">{{ runningTag }}</span
          >. After switching you'll be offered the newest release build, which
          may be older than what you have.
        </p>
        <div class="mt-3 flex gap-2">
          <button
            type="button"
            class="rounded-md bg-yellow-500/20 px-3 py-1.5 text-xs font-semibold text-yellow-100 hover:bg-yellow-500/30"
            @click="() => saveBranch(pendingBranch!)"
          >
            Switch to Release
          </button>
          <button
            type="button"
            class="rounded-md px-3 py-1.5 text-xs font-semibold text-yellow-100/80 hover:bg-yellow-500/10"
            @click="pendingBranch = undefined"
          >
            Stay on Test
          </button>
        </div>
      </div>
    </div>

    <!-- auto check -->
    <div
      class="mt-8 flex max-w-3xl items-start justify-between gap-6 border-t border-zinc-800 pt-6"
    >
      <div>
        <h4 class="text-sm font-medium text-zinc-100">
          Tell me about updates when Drop starts
        </h4>
        <p class="mt-1 text-sm text-zinc-400">
          When off, Drop only shows updates after you press “Check now”.
          Updates your server marks as required are always shown.
        </p>
      </div>
      <Switch
        v-model="checkOnStart"
        :class="[
          checkOnStart ? 'bg-blue-600' : 'bg-zinc-700',
          'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
        ]"
      >
        <span class="sr-only">Tell me about updates when Drop starts</span>
        <span
          :class="[
            checkOnStart ? 'translate-x-5' : 'translate-x-0',
            'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
          ]"
        />
      </Switch>
    </div>
  </template>
</template>

<script setup lang="ts">
import {
  RadioGroup,
  RadioGroupDescription,
  RadioGroupLabel,
  RadioGroupOption,
  Switch,
} from "@headlessui/vue";
import {
  ArrowPathIcon,
  ArrowUpCircleIcon,
  CheckIcon,
  ExclamationTriangleIcon,
} from "@heroicons/vue/24/outline";
import type { ClientUpdateStatus, Settings, UpdateBranch } from "~/types";

const status = useClientUpdateStatus();
const dialog = useClientUpdateDialog();

if (!status.value)
  status.value = await invokeWithTimeout<ClientUpdateStatus>(
    "fetch_client_update_status",
  );

const settings = await invokeWithTimeout<Settings>("fetch_settings");
const branch = ref<UpdateBranch>(settings.updateBranch);
const checkOnStart = ref(settings.checkUpdatesOnStart);
const pendingBranch = ref<UpdateBranch | undefined>();

const branchOptions = [
  {
    value: "release" as const,
    label: "Release",
    description: "Finished builds only. Recommended for most people.",
  },
  {
    value: "test" as const,
    label: "Test",
    description:
      "The newest build on either branch, including test builds made from uncommitted changes. Expect bugs.",
  },
];

const runningTag = computed(
  () => status.value?.current?.tag ?? status.value?.appVersion,
);
const onTestBuild = computed(
  () =>
    status.value?.current?.branchSlug === "test" ||
    /[.-]dirty\b/.test(status.value?.appVersion ?? ""),
);

const card = computed(() => {
  const s = status.value!;
  if (s.installed)
    return {
      title: `Drop ${s.installed} is installed. Restart to use it.`,
      icon: CheckIcon,
      iconClass: "bg-green-500/15 text-green-400",
    };
  if (s.update)
    return {
      title: s.update.rollback
        ? `Drop ${s.update.tag} is available on this branch`
        : `Drop ${s.update.tag} is available`,
      icon: ArrowUpCircleIcon,
      iconClass: "bg-blue-500/15 text-blue-400",
    };
  if (s.error || !s.lastChecked)
    return {
      title: s.error ? "Couldn't check for updates" : "Updates",
      icon: ArrowPathIcon,
      iconClass: "bg-zinc-700/60 text-zinc-400",
    };
  return {
    title: "You're up to date",
    icon: CheckIcon,
    iconClass: "bg-green-500/15 text-green-400",
  };
});

async function check() {
  try {
    await checkForClientUpdate();
  } catch (e) {
    console.error("update check failed", e);
  }
}

function chooseBranch(value: UpdateBranch) {
  if (value === branch.value) {
    pendingBranch.value = undefined;
    return;
  }
  // Leaving test while running a test build can mean going backwards; say so first
  if (value === "release" && onTestBuild.value) {
    pendingBranch.value = value;
    return;
  }
  saveBranch(value);
}

async function saveBranch(value: UpdateBranch) {
  pendingBranch.value = undefined;
  const previous = branch.value;
  branch.value = value;
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { updateBranch: value },
    });
  } catch (error) {
    console.error("Failed to update branch setting:", error);
    branch.value = previous;
    return;
  }
  await check();
}

watch(checkOnStart, async (newValue) => {
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { checkUpdatesOnStart: newValue },
    });
  } catch (error) {
    console.error("Failed to update check-on-start setting:", error);
    checkOnStart.value = !newValue;
  }
});

function formatTime(date: string) {
  const value = new Date(date);
  const today = new Date().toDateString() === value.toDateString();
  const time = value.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return today ? `today at ${time}` : `${value.toLocaleDateString()} ${time}`;
}
</script>
