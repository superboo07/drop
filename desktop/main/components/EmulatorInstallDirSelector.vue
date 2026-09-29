<template>
  <Listbox as="div" v-model="selection">
    <div class="relative">
      <ListboxButton
        class="relative w-full cursor-default rounded-md bg-zinc-800 py-1.5 pl-3 pr-10 text-left text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 focus:outline-none focus:ring-2 focus:ring-blue-600 sm:text-sm/6"
      >
        <span v-if="model" class="block truncate">{{ model }}</span>
        <span v-else class="block truncate text-zinc-400"
          >First install directory
          <span v-if="installDirs[0]" class="text-zinc-500"
            >({{ installDirs[0] }})</span
          ></span
        >
        <span
          class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2"
        >
          <ChevronUpDownIcon class="h-5 w-5 text-gray-400" aria-hidden="true" />
        </span>
      </ListboxButton>

      <transition
        leave-active-class="transition ease-in duration-100"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <ListboxOptions
          class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-md bg-zinc-900 py-1 text-base shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none sm:text-sm"
        >
          <ListboxOption
            as="template"
            v-for="option in options"
            :key="option.value"
            :value="option.value"
            v-slot="{ active, selected }"
          >
            <li
              :class="[
                active ? 'bg-blue-600 text-white' : 'text-zinc-300',
                option.value === ADD && !active ? 'text-blue-400' : '',
                option.divider ? 'border-t border-zinc-800' : '',
                'relative cursor-default select-none py-2 pl-3 pr-9',
              ]"
            >
              <span
                :class="[
                  selected ? 'font-semibold text-zinc-100' : 'font-normal',
                  'block truncate',
                ]"
                >{{ option.label }}</span
              >
              <span
                v-if="selected"
                :class="[
                  active ? 'text-white' : 'text-blue-600',
                  'absolute inset-y-0 right-0 flex items-center pr-4',
                ]"
              >
                <CheckIcon class="h-5 w-5" aria-hidden="true" />
              </span>
            </li>
          </ListboxOption>
        </ListboxOptions>
      </transition>
    </div>
  </Listbox>
</template>

<script setup lang="ts">
import {
  Listbox,
  ListboxButton,
  ListboxOption,
  ListboxOptions,
} from "@headlessui/vue";
import { CheckIcon, ChevronUpDownIcon } from "@heroicons/vue/20/solid";

// Listbox values can't be null, so "" stands for the first install
// directory, and ADD for the "add a new directory" entry.
const ADD = "\0add";

const { installDirs } = defineProps<{ installDirs: string[] }>();
const model = defineModel<string | null>({ required: true });
const emit = defineEmits<{ add: [] }>();

const options = computed(() => [
  {
    value: "",
    label: `First install directory${installDirs[0] ? ` (${installDirs[0]})` : ""}`,
    divider: false,
  },
  ...installDirs.map((dir, index) => ({
    value: dir,
    label: dir,
    divider: index === 0,
  })),
  { value: ADD, label: "Add a new directory…", divider: true },
]);

const selection = computed({
  get: () => model.value ?? "",
  set: (value: string) => {
    if (value === ADD) emit("add");
    else model.value = value || null;
  },
});
</script>
