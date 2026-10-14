<template>
  <div class="flex flex-col">
    <!-- tabs-->
    <div>
      <div class="border-b border-white/10">
        <nav class="-mb-px flex gap-x-2" aria-label="Tabs">
          <NuxtLink
            v-for="(tab, tabIdx) in navigation"
            :key="tab.route"
            :href="tab.route"
            :class="[
              currentNavigationIndex == tabIdx
                ? 'border-blue-400 text-blue-400'
                : 'border-transparent text-gray-400 hover:border-white/20 hover:text-gray-300',
              'group inline-flex items-center border-b-2 px-1 py-4 text-sm font-medium',
            ]"
            :aria-current="tab ? 'page' : undefined"
          >
            <component
              :is="tab.icon"
              :class="[
                currentNavigationIndex == tabIdx
                  ? 'text-blue-400'
                  : 'text-gray-500 group-hover:text-gray-400',
                'mr-2 -ml-0.5 size-5',
              ]"
              aria-hidden="true"
            />
            <span>{{ tab.label }}</span>
          </NuxtLink>
        </nav>
      </div>
    </div>
    <!-- content -->
    <div class="mt-4 grow flex">
      <NuxtPage />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ArrowPathIcon, DocumentTextIcon } from "@heroicons/vue/24/outline";

const navigation: Array<NavigationItem & { icon: Component }> = [
  {
    label: $t("header.admin.clients.logs"),
    route: "/admin/clients/logs",
    prefix: "/admin/clients/logs",
    icon: DocumentTextIcon,
  },
  {
    label: $t("header.admin.clients.updater"),
    route: "/admin/clients/updater",
    prefix: "/admin/clients/updater",
    icon: ArrowPathIcon,
  },
];

const currentNavigationIndex = useCurrentNavigationIndex(navigation);
</script>
