import { listen } from "@tauri-apps/api/event";
import type { WinetricksJob, WinetricksVerbProgress } from "~/types";

/** Each game's winetricks job (running, or finished and not yet dismissed) */
export const useWinetricksJobs = () =>
  useState<Record<string, WinetricksJob>>("winetricks-jobs", () => ({}));

/** Jobs that just finished, for the toasts */
export const useWinetricksFinished = () =>
  useState<WinetricksJob[]>("winetricks-finished", () => []);

/**
 * The game whose Proton options are open, if any: its result is shown there,
 * so it doesn't also need a toast
 */
export const useWinetricksViewing = () =>
  useState<string | undefined>("winetricks-viewing", () => undefined);

listen<WinetricksJob>("winetricks/update", (event) => {
  const jobs = useWinetricksJobs();
  const previous = jobs.value[event.payload.gameId];
  jobs.value = { ...jobs.value, [event.payload.gameId]: event.payload };
  if (
    previous?.state === "Running" &&
    event.payload.state !== "Running" &&
    event.payload.state !== "Cancelled" &&
    useWinetricksViewing().value !== event.payload.gameId
  ) {
    useWinetricksFinished().value = [
      ...useWinetricksFinished().value.filter(
        (job) => job.gameId !== event.payload.gameId,
      ),
      event.payload,
    ];
  }
});

listen<string>("winetricks/removed", (event) => {
  const { [event.payload]: _, ...rest } = useWinetricksJobs().value;
  useWinetricksJobs().value = rest;
});

// Jobs started before this window loaded. The commands only exist on Linux.
invokeWithTimeout<WinetricksJob[]>("fetch_winetricks_jobs")
  .then((jobs) => {
    useWinetricksJobs().value = Object.fromEntries(
      jobs.map((job) => [job.gameId, job]),
    );
  })
  .catch(() => {});

export const useWinetricksJob = (gameId: MaybeRefOrGetter<string>) => {
  const jobs = useWinetricksJobs();
  return computed(() => jobs.value[toValue(gameId)]);
};

/** The running verb, or the next one to run */
export function currentVerb(job: WinetricksJob) {
  return (
    job.verbs.find((verb) => verb.state === "Running") ??
    job.verbs.find((verb) => verb.state === "Waiting")
  );
}

export function verbIsDone(verb: WinetricksVerbProgress) {
  return verb.state === "Installed" || verb.state === "AlreadyInstalled";
}

/** "2 of 3": the running verb's place in the job */
export function verbPosition(job: WinetricksJob) {
  const verb = currentVerb(job);
  if (!verb) return "";
  return `${job.verbs.indexOf(verb) + 1} of ${job.verbs.length}`;
}

/** Fraction of the job's verbs that are done, counting the running one as half */
export function jobFraction(job: WinetricksJob) {
  if (job.verbs.length === 0) return 0;
  const done = job.verbs.filter(verbIsDone).length;
  const running = job.verbs.find((verb) => verb.state === "Running");
  const partial = running ? (running.percent ?? 50) / 100 : 0;
  return (done + partial * 0.9) / job.verbs.length;
}

export function formatDuration(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

/** A clock that ticks every second while the page needs it */
export function useNow() {
  const now = ref(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;
  onMounted(() => {
    timer = setInterval(() => (now.value = Date.now()), 1000);
  });
  onUnmounted(() => clearInterval(timer));
  return now;
}

export async function installWinetricks(gameId: string, verbs: string[]) {
  await invokeWithTimeout("install_winetricks_verbs", { gameId, verbs });
}

export async function cancelWinetricks(gameId: string) {
  await invokeWithTimeout("cancel_winetricks", { gameId });
}

export async function dismissWinetricks(gameId: string) {
  useWinetricksFinished().value = useWinetricksFinished().value.filter(
    (job) => job.gameId !== gameId,
  );
  await invokeWithTimeout("dismiss_winetricks", { gameId });
}

export async function openWinetricksLog(gameId: string) {
  await invokeWithTimeout("open_winetricks_log", { gameId });
}
