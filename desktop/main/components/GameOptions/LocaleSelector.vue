<template>
  <Listbox as="div" v-model="model.locale" class="mt-6">
    <ListboxLabel class="block text-sm/6 font-medium text-white"
      >Locale<RecommendedBadge :state="recommendation"
    /></ListboxLabel>
    <div class="relative mt-2">
      <ListboxButton
        class="grid w-full cursor-default grid-cols-1 rounded-md bg-white/5 py-1.5 pr-2 pl-3 text-left text-white outline-1 -outline-offset-1 outline-white/10 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-blue-500 sm:text-sm/6"
      >
        <span v-if="currentLocale" class="col-start-1 row-start-1 truncate pr-6"
          >{{ currentLocale.name }}
          <span class="text-zinc-400">({{ currentLocale.id }})</span></span
        >
        <span
          v-else
          class="col-start-1 row-start-1 truncate pr-6 italic text-zinc-400"
          >System default</span
        >
        <ChevronUpDownIcon
          class="col-start-1 row-start-1 size-5 self-center justify-self-end text-zinc-400 sm:size-4"
          aria-hidden="true"
        />
      </ListboxButton>

      <transition
        leave-active-class="transition ease-in duration-100"
        leave-from-class=""
        leave-to-class="opacity-0"
      >
        <ListboxOptions
          class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-md bg-zinc-800 py-1 text-base outline-1 -outline-offset-1 outline-white/10 sm:text-sm"
        >
          <ListboxOption
            as="template"
            :value="null"
            v-slot="{ active, selected }"
          >
            <li
              :class="[
                active ? 'bg-blue-500 text-white outline-hidden' : 'text-white',
                'relative cursor-default py-2 pr-9 pl-3 select-none',
              ]"
            >
              <span
                :class="[
                  selected ? 'font-semibold' : 'font-normal',
                  'block truncate italic',
                ]"
                >System default</span
              >
              <span class="block truncate text-xs text-zinc-400"
                >Run the game with this computer's own locale.</span
              >

              <span
                v-if="selected"
                :class="[
                  active ? 'text-white' : 'text-blue-400',
                  'absolute inset-y-0 right-0 flex items-center pr-4',
                ]"
              >
                <CheckIcon class="size-5" aria-hidden="true" />
              </span>
            </li>
          </ListboxOption>
          <ListboxOption
            as="template"
            v-for="locale in locales"
            :key="locale.id"
            :value="locale.id"
            v-slot="{ active, selected }"
          >
            <li
              :class="[
                active ? 'bg-blue-500 text-white outline-hidden' : 'text-white',
                'relative cursor-default py-2 pr-9 pl-3 select-none',
              ]"
            >
              <span
                :class="[
                  selected ? 'font-semibold' : 'font-normal',
                  'block truncate',
                ]"
                >{{ locale.name }}</span
              >
              <span class="block truncate text-xs text-zinc-400">{{
                locale.id
              }}</span>

              <span
                v-if="selected"
                :class="[
                  active ? 'text-white' : 'text-blue-400',
                  'absolute inset-y-0 right-0 flex items-center pr-4',
                ]"
              >
                <CheckIcon class="size-5" aria-hidden="true" />
              </span>
            </li>
          </ListboxOption>
        </ListboxOptions>
      </transition>
    </div>
    <p class="mt-2 text-sm text-zinc-400">
      Run this game under a different locale — e.g. Japanese for visual novels
      that show garbled text otherwise.
    </p>
    <p
      v-if="notInstalled"
      class="mt-2 rounded-md bg-zinc-500/10 p-3 text-sm text-zinc-400 outline outline-zinc-500/20"
    >
      <span class="font-semibold text-zinc-200">{{ model.locale }}</span> isn't
      generated on this system. Games run through Proton will still use it, but
      native Linux games need it enabled first (e.g. in
      <code>/etc/locale.gen</code>, then <code>locale-gen</code>).
    </p>
  </Listbox>
</template>

<script setup lang="ts">
import {
  Listbox,
  ListboxButton,
  ListboxLabel,
  ListboxOption,
  ListboxOptions,
} from "@headlessui/vue";
import { ChevronUpDownIcon } from "@heroicons/vue/16/solid";
import { CheckIcon } from "@heroicons/vue/20/solid";
import type { GameVersion } from "~/types";
import RecommendedBadge from "./RecommendedBadge.vue";
import {
  PROTON_DEFAULTS_KEY,
  recommendationState,
} from "~/composables/proton-defaults";

const model = defineModel<GameVersion["userConfiguration"]>({ required: true });

const protonDefaults = inject(PROTON_DEFAULTS_KEY, null);
const recommendation = computed(() =>
  recommendationState<string | null>(
    protonDefaults?.locale,
    model.value.locale,
    (v) => v ?? "",
  ),
);

// Locales games are commonly region-locked to, listed first regardless of
// whether the host has them generated.
const commonLocales: Array<{ id: string; name: string }> = [
  { id: "ja_JP.UTF-8", name: "Japanese (Japan)" },
  { id: "zh_CN.UTF-8", name: "Chinese (Simplified)" },
  { id: "zh_TW.UTF-8", name: "Chinese (Traditional)" },
  { id: "ko_KR.UTF-8", name: "Korean (South Korea)" },
  { id: "en_US.UTF-8", name: "English (United States)" },
  { id: "en_GB.UTF-8", name: "English (United Kingdom)" },
  { id: "ru_RU.UTF-8", name: "Russian (Russia)" },
  { id: "de_DE.UTF-8", name: "German (Germany)" },
  { id: "fr_FR.UTF-8", name: "French (France)" },
  { id: "es_ES.UTF-8", name: "Spanish (Spain)" },
  { id: "it_IT.UTF-8", name: "Italian (Italy)" },
  { id: "pt_BR.UTF-8", name: "Portuguese (Brazil)" },
  { id: "pl_PL.UTF-8", name: "Polish (Poland)" },
];

const systemLocales = ref<string[] | undefined>();
invokeWithTimeout<string[]>("list_system_locales")
  .then((result) => (systemLocales.value = result))
  .catch(() => {
    // Only used for the "not generated" hint; the picker works without it.
  });

const locales = computed(() => {
  const known = new Set(commonLocales.map((v) => v.id));
  const extra = (systemLocales.value ?? [])
    .filter(
      (v) => !known.has(v) && v !== "C" && !v.startsWith("C.") && v !== "POSIX",
    )
    .map((id) => ({ id, name: id }));
  // Keep a previously saved locale selectable even if it's in neither list.
  if (
    model.value.locale &&
    !known.has(model.value.locale) &&
    !extra.some((v) => v.id === model.value.locale)
  ) {
    extra.unshift({ id: model.value.locale, name: model.value.locale });
  }
  return [...commonLocales, ...extra];
});

const currentLocale = computed(() =>
  locales.value.find((v) => v.id === model.value.locale),
);

const notInstalled = computed(
  () =>
    !!model.value.locale &&
    !!systemLocales.value?.length &&
    !systemLocales.value.includes(model.value.locale),
);
</script>
