<template>
  <ModalTemplate v-model="open" size-class="sm:max-w-2xl">
    <template #default>
      <div class="flex flex-col gap-y-4">
        <div>
          <h1 class="text-lg font-semibold font-display text-zinc-100">
            {{
              $t("library.admin.version.proton.title", [
                version.displayName ?? version.versionPath ?? "",
              ])
            }}
          </h1>
          <p class="mt-1 text-sm text-zinc-400">
            {{ $t("library.admin.version.proton.description") }}
          </p>
        </div>

        <div class="bg-zinc-800 p-4 rounded-xl flex flex-col gap-y-2">
          <div>
            <label
              for="proton-name"
              class="block text-sm font-medium leading-6 text-zinc-100"
              >{{ $t("library.admin.version.proton.preferredTitle") }}</label
            >
            <p class="text-zinc-400 text-xs">
              {{ $t("library.admin.version.proton.preferredDesc") }}
            </p>
          </div>
          <input
            id="proton-name"
            v-model="form.protonName"
            type="text"
            :class="inputClass"
            :placeholder="
              $t('library.admin.version.proton.preferredPlaceholder')
            "
          />
        </div>

        <div class="bg-zinc-800 p-4 rounded-xl flex flex-col gap-y-3">
          <span class="block text-sm font-medium leading-6 text-zinc-100">{{
            $t("library.admin.version.proton.compatTitle")
          }}</span>
          <div
            v-for="toggle in toggles"
            :key="toggle.key"
            class="flex flex-wrap items-center justify-between gap-3"
          >
            <div class="min-w-0 flex-1">
              <p class="text-sm font-medium text-zinc-100">
                {{ toggle.title }}
              </p>
              <p class="text-xs text-zinc-400">{{ toggle.desc }}</p>
            </div>
            <div
              role="group"
              :aria-label="toggle.title"
              class="inline-flex rounded-md bg-zinc-950 p-0.5 ring-1 ring-zinc-700"
            >
              <button
                v-for="option in triState"
                :key="String(option.value)"
                type="button"
                :aria-pressed="form[toggle.key] === option.value"
                :class="[
                  'rounded px-2.5 py-1 text-xs transition',
                  form[toggle.key] === option.value
                    ? option.value === null
                      ? 'bg-zinc-700 text-zinc-100'
                      : 'bg-blue-600 text-white'
                    : 'text-zinc-400 hover:text-zinc-200',
                ]"
                @click="form[toggle.key] = option.value"
              >
                {{ option.label }}
              </button>
            </div>
          </div>
        </div>

        <div class="bg-zinc-800 p-4 rounded-xl flex flex-col gap-y-2">
          <div>
            <label
              for="proton-locale"
              class="block text-sm font-medium leading-6 text-zinc-100"
              >{{ $t("library.admin.version.proton.localeTitle") }}</label
            >
            <p class="text-zinc-400 text-xs">
              {{ $t("library.admin.version.proton.localeDesc") }}
            </p>
          </div>
          <select id="proton-locale" v-model="form.locale" :class="inputClass">
            <option value="">
              {{ $t("library.admin.version.proton.noPreference") }}
            </option>
            <option
              v-for="locale in localeOptions"
              :key="locale"
              :value="locale"
            >
              {{ localeName(locale) }}
            </option>
            <option :value="OTHER_LOCALE">
              {{ $t("library.admin.version.proton.localeOther") }}
            </option>
          </select>
          <input
            v-if="form.locale === OTHER_LOCALE"
            v-model="form.customLocale"
            type="text"
            :aria-label="$t('library.admin.version.proton.localeTitle')"
            :class="inputClass"
            :placeholder="
              $t('library.admin.version.proton.localeOtherPlaceholder')
            "
          />
        </div>

        <div class="bg-zinc-800 p-4 rounded-xl flex flex-col gap-y-2">
          <div>
            <label
              for="proton-env"
              class="block text-sm font-medium leading-6 text-zinc-100"
              >{{ $t("library.admin.version.proton.envTitle") }}</label
            >
            <p class="text-zinc-400 text-xs">
              {{ $t("library.admin.version.proton.envDesc") }}
            </p>
          </div>
          <textarea
            id="proton-env"
            v-model="form.extraEnvVars"
            rows="3"
            :class="[inputClass, 'font-mono']"
            :placeholder="ENV_PLACEHOLDER"
          />
        </div>

        <div class="bg-zinc-800 p-4 rounded-xl flex flex-col gap-y-2">
          <div>
            <label
              for="proton-winetricks"
              class="block text-sm font-medium leading-6 text-zinc-100"
              >{{ $t("library.admin.version.proton.winetricksTitle") }}</label
            >
            <p class="text-zinc-400 text-xs">
              {{ $t("library.admin.version.proton.winetricksDesc") }}
            </p>
          </div>
          <div v-if="form.winetricks.length > 0" class="flex flex-wrap gap-1.5">
            <span
              v-for="(verb, idx) in form.winetricks"
              :key="verb"
              class="inline-flex items-center gap-x-1 rounded-md bg-zinc-950 py-0.5 pl-2 pr-1 font-mono text-xs text-zinc-200 ring-1 ring-zinc-700"
            >
              {{ verb }}
              <button
                type="button"
                class="rounded p-0.5 text-zinc-500 hover:text-red-500"
                :aria-label="$t('common.remove')"
                @click="form.winetricks.splice(idx, 1)"
              >
                <XMarkIcon class="size-3.5" />
              </button>
            </span>
          </div>
          <input
            id="proton-winetricks"
            v-model="verbInput"
            type="text"
            :class="[inputClass, 'font-mono']"
            :placeholder="
              $t('library.admin.version.proton.winetricksPlaceholder')
            "
            @keydown="onVerbKey"
            @blur="addVerbs"
          />
        </div>

        <div v-if="error" class="w-fit rounded-md bg-red-600/10 p-4">
          <div class="flex">
            <div class="flex-shrink-0">
              <XCircleIcon class="h-5 w-5 text-red-600" aria-hidden="true" />
            </div>
            <div class="ml-3">
              <h3 class="text-sm font-medium text-red-600">{{ error }}</h3>
            </div>
          </div>
        </div>
      </div>
    </template>
    <template #buttons>
      <LoadingButton
        type="button"
        :loading="saving"
        class="inline-flex w-full shadow-sm sm:ml-3 sm:w-auto"
        @click="() => save()"
      >
        {{ $t("common.save") }}
      </LoadingButton>
      <button
        type="button"
        class="mt-3 inline-flex w-full justify-center rounded-md bg-zinc-900 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-950 sm:mt-0 sm:w-auto"
        @click="open = false"
      >
        {{ $t("cancel") }}
      </button>
      <button
        type="button"
        class="mt-3 inline-flex w-full justify-center rounded-md px-3 py-2 text-sm font-semibold text-zinc-400 hover:text-zinc-100 sm:mt-0 sm:mr-auto sm:w-auto"
        @click="clearAll"
      >
        {{ $t("library.admin.version.proton.clear") }}
      </button>
    </template>
  </ModalTemplate>
</template>

<script setup lang="ts">
import { XMarkIcon } from "@heroicons/vue/20/solid";
import { XCircleIcon } from "@heroicons/vue/16/solid";
import type { H3Error } from "h3";
import type { SerializeObject } from "nitropack";
import type { AdminFetchGameType } from "~/server/api/v1/admin/game/[id]/index.get";

const open = defineModel<boolean>({ required: true });

const props = defineProps<{
  gameId: string;
  version: SerializeObject<AdminFetchGameType>["versions"][number];
}>();

const emit = defineEmits<{ saved: [] }>();

const { t, locale: uiLocale } = useI18n();

const inputClass =
  "block w-full rounded-md bg-zinc-950 px-3 py-1.5 text-zinc-100 outline-1 -outline-offset-1 outline-zinc-800 placeholder:text-zinc-500 focus:outline-1 focus:-outline-offset-1 focus:outline-blue-500 sm:text-sm/6";

const ENV_PLACEHOLDER = "WINEDLLOVERRIDES=dinput8=n,b\nDXVK_ASYNC=1";
const OTHER_LOCALE = "__other";

// The same region-locked locales the desktop client lists first
const localeOptions = [
  "ja_JP.UTF-8",
  "zh_CN.UTF-8",
  "zh_TW.UTF-8",
  "ko_KR.UTF-8",
  "en_US.UTF-8",
  "en_GB.UTF-8",
  "ru_RU.UTF-8",
  "de_DE.UTF-8",
  "fr_FR.UTF-8",
  "es_ES.UTF-8",
  "it_IT.UTF-8",
  "pt_BR.UTF-8",
  "pl_PL.UTF-8",
];

function localeName(locale: string) {
  const tag = locale.split(".")[0].replace("_", "-");
  try {
    const name = new Intl.DisplayNames([uiLocale.value.replace("_", "-")], {
      type: "language",
    }).of(tag);
    return name ? `${name} (${locale})` : locale;
  } catch {
    return locale;
  }
}

type TriState = boolean | null;
type ToggleKey = "dxvk" | "esync" | "fsync";

const toggles = computed<
  Array<{ key: ToggleKey; title: string; desc: string }>
>(() => [
  {
    key: "dxvk",
    title: t("library.admin.version.proton.dxvkTitle"),
    desc: t("library.admin.version.proton.dxvkDesc"),
  },
  {
    key: "esync",
    title: t("library.admin.version.proton.esyncTitle"),
    desc: t("library.admin.version.proton.esyncDesc"),
  },
  {
    key: "fsync",
    title: t("library.admin.version.proton.fsyncTitle"),
    desc: t("library.admin.version.proton.fsyncDesc"),
  },
]);

const triState = computed<Array<{ value: TriState; label: string }>>(() => [
  { value: null, label: t("library.admin.version.proton.noPreference") },
  { value: true, label: t("library.admin.version.proton.on") },
  { value: false, label: t("library.admin.version.proton.off") },
]);

interface ProtonForm {
  protonName: string;
  dxvk: TriState;
  esync: TriState;
  fsync: TriState;
  locale: string;
  customLocale: string;
  extraEnvVars: string;
  winetricks: string[];
}

function buildForm(): ProtonForm {
  const d = props.version.protonDefaults;
  const locale = d?.locale ?? "";
  const known = locale === "" || localeOptions.includes(locale);
  return {
    protonName: d?.protonName ?? "",
    dxvk: d?.dxvk ?? null,
    esync: d?.esync ?? null,
    fsync: d?.fsync ?? null,
    locale: known ? locale : OTHER_LOCALE,
    customLocale: known ? "" : locale,
    extraEnvVars: d?.extraEnvVars ?? "",
    winetricks: [...(d?.winetricks ?? [])],
  };
}

const form = ref<ProtonForm>(buildForm());
const verbInput = ref("");

watch(open, (isOpen) => {
  if (isOpen) {
    form.value = buildForm();
    verbInput.value = "";
    error.value = undefined;
  }
});

function addVerbs() {
  for (const verb of verbInput.value.split(/[\s,]+/)) {
    if (verb && !form.value.winetricks.includes(verb))
      form.value.winetricks.push(verb);
  }
  verbInput.value = "";
}

function onVerbKey(e: KeyboardEvent) {
  if (e.key !== "Enter" && e.key !== ",") return;
  e.preventDefault();
  addVerbs();
}

function clearAll() {
  form.value = {
    protonName: "",
    dxvk: null,
    esync: null,
    fsync: null,
    locale: "",
    customLocale: "",
    extraEnvVars: "",
    winetricks: [],
  };
  verbInput.value = "";
}

const saving = ref(false);
const error = ref<string | undefined>();

async function save() {
  addVerbs();
  saving.value = true;
  error.value = undefined;
  const locale =
    form.value.locale === OTHER_LOCALE
      ? form.value.customLocale
      : form.value.locale;
  try {
    await $dropFetch("/api/v1/admin/game/:id/versions/proton", {
      method: "PATCH",
      params: { id: props.gameId },
      body: {
        versionId: props.version.versionId,
        protonName: form.value.protonName || null,
        dxvk: form.value.dxvk,
        esync: form.value.esync,
        fsync: form.value.fsync,
        locale: locale || null,
        extraEnvVars: form.value.extraEnvVars || null,
        winetricks: form.value.winetricks,
      },
    });
    open.value = false;
    emit("saved");
  } catch (e) {
    error.value = (e as H3Error)?.statusMessage ?? t("errors.unknown");
  } finally {
    saving.value = false;
  }
}
</script>
