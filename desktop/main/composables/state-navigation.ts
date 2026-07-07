import { listen } from "@tauri-apps/api/event";
import { data } from "autoprefixer";
import { AppStatus, type AppState } from "~/types";

export function setupHooks() {
  const router = useRouter();
  const state = useAppState();

  listen("auth/processing", (event) => {
    router.push("/auth/processing").catch((e) => {
      console.error("router.push to /auth/processing failed", e);
    });
  });

  listen("auth/failed", (event) => {
    router
      .push(`/auth/failed?error=${encodeURIComponent(event.payload as string)}`)
      .catch((e) => {
        console.error("router.push to /auth/failed failed", e);
      });
  });

  listen("auth/finished", async (event) => {
    console.log("auth/finished received, fetching state and navigating");
    state.value = JSON.parse(await invokeWithTimeout("fetch_state"));
    try {
      await router.push("/library");
      console.log("navigated to /library");
    } catch (e) {
      console.error("router.push to /library failed", e);
    }
  });

  listen("download_error", (event) => {
    createModal(
      ModalType.Notification,
      {
        title: "Drop encountered an error while downloading",
        description: `Drop encountered an error while downloading your game: "${(
          event.payload as unknown as string
        ).toString()}"`,
        buttonText: "Close",
      },
      (e, c) => c()
    );
  });

  // This is for errors that (we think) aren't our fault
  listen("launch_external_error", (event) => {
    createModal(
      ModalType.Confirmation,
      {
        title: "Did something go wrong?",
        description:
          "Drop detected that something might've gone wrong with launching your game. Do you want to open the log directory?",
        buttonText: "Open",
      },
      async (e, c) => {
        if (e == "confirm") {
          try {
            await invokeWithTimeout("open_process_logs", {
              gameId: event.payload,
            });
          } catch (err) {
            createModal(
              ModalType.Notification,
              {
                title: "Couldn't open the log directory",
                description: `Drop failed to open the log directory: "${err}"`,
                buttonText: "Close",
              },
              (e, c) => c()
            );
          }
        }
        c();
      }
    );
  });

  /*

  document.addEventListener("contextmenu", (event) => {
    event.target?.dispatchEvent(new Event("contextmenu"));
    event.preventDefault();
  });

  */
}

export async function initialNavigation(state: ReturnType<typeof useAppState>) {
  if (!state.value)
    throw createError({
      statusCode: 500,
      statusMessage: "App state not valid",
      fatal: true,
    });
  const router = useRouter();

  let target: string | { path: string };
  switch (state.value.status) {
    case AppStatus.NotConfigured:
      target = { path: "/setup" };
      break;
    case AppStatus.SignedOut:
      target = "/auth";
      break;
    case AppStatus.SignedInNeedsReauth:
      target = "/auth/signedout";
      break;
    case AppStatus.ServerUnavailable:
      target = "/error/serverunavailable";
      break;
    default:
      target = "/library";
  }

  console.log(`initialNavigation navigating to ${JSON.stringify(target)}`);
  try {
    await router.push(target);
    console.log(`initialNavigation reached ${JSON.stringify(target)}`);
  } catch (e) {
    console.error(
      `initialNavigation: router.push to ${JSON.stringify(target)} failed`,
      e,
    );
  }
}
