import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

// Checks GitHub once at startup. No document content is ever sent; failures
// (offline, GitHub unreachable) are silent so the app keeps working offline.
export async function offerUpdate(banner) {
  let update;
  try {
    update = await check();
  } catch {
    return;
  }
  if (!update) return;

  const text = banner.querySelector("span");
  const button = banner.querySelector("button");
  text.textContent = `Version ${update.version} is available.`;
  banner.hidden = false;

  button.addEventListener("click", async () => {
    button.disabled = true;
    let total = 0;
    let received = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          received += event.data.chunkLength;
          text.textContent = total
            ? `Downloading update… ${Math.round((received / total) * 100)}%`
            : "Downloading update…";
        }
        if (event.event === "Finished") text.textContent = "Installing update…";
      });
      await relaunch();
    } catch (error) {
      text.textContent = `The update could not be installed: ${error}`;
      button.disabled = false;
    }
  });
}
