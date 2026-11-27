import { invoke } from "@tauri-apps/api/core";
import type { Store } from "@tauri-apps/plugin-store";

export const ANISETTE_SERVER_KEY = "anisetteServer";
export const DEFAULT_ANISETTE_SERVER = "new.ani.sidestore.io";

/**
 * Presets offered in Settings: the same servers, in the same order, as the list
 * the store app reads from loveipa.com/anisette-servers.json.
 *
 * Apple no longer sees the server's own identity (see
 * src-tauri/src/anisette_identity.rs), but the server still has to be able to
 * register a device, and since Nov 2026 Apple refuses every Android-ADI server
 * at that step with -29003 ("possible environment mismatch"). These are Mac-
 * or Windows-ADI servers; each passed provisioning on 2026-11-27.
 */
export const ANISETTE_SERVERS: [host: string, label: string][] = [
  [DEFAULT_ANISETTE_SERVER, "SideStore (New)"],
  ["ani.stikstore.app", "StikStore"],
  ["mac-ani.jkcoxson.com", "jkcoxson"],
  ["ani.jkcoxson.com", "jkcoxson (Windows ADI)"],
  ["anisette.lillq.me", "Lill q"],
];

/**
 * Servers dropped from the presets. Removing one from the list alone would
 * strand whoever picked it: the setting persists, and the dropdown shows an
 * unknown value as a custom server. See `migrateRetiredAnisetteServer`.
 */
const RETIRED_ANISETTE_SERVERS = new Set([
  // 1.0.6: Android-ADI servers, refused by Apple at provisioning (-29003). They
  // also report a `<macOS;` identity, which Apple has answered with 503 since
  // 2026-11-20. The first two were our presets; the rest are on SideStore's
  // public list, so someone may have typed them in as a custom server.
  "ani.sidestore.io",
  "ani.sidestore.app",
  "ani.npeg.us",
  "ani.uwucloud.fun",
  "ani.sjwon0368.dpdns.org",
  "ani.idevicehacked.com",
  "ani3server.fly.dev",
  "ani.jaydenha.uk",
  "anisette.crystall1ne.dev",
  "omni.ethxn.xyz",
  // 1.0.5: reported the Xcode identity, refused at sign-in since Sept 2026.
  "ani.sidestore.zip",
  "ani.846969.xyz",
  "ani.xu30.top",
  "anisette.wedotstud.io",
  "ani.neoarz.xyz", // domain lapsed, now a parked page listed for sale
]);

const hostOf = (server: string) =>
  server
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, "")
    .replace(/\/+$/, "");

let migratedFrom: string | null = null;

/**
 * Moves the saved setting off a retired server. Runs before the app renders,
 * so no sign-in can go out through the old one first.
 *
 * The anisette state needs clearing too, since it was provisioned at the old
 * server, but that no longer happens here: the backend tags the state with the
 * server and identity it was provisioned for and clears it whenever either
 * changes (`prepare_anisette`, and every sign-in).
 */
export async function migrateRetiredAnisetteServer(store: Store): Promise<void> {
  const current = await store.get<string>(ANISETTE_SERVER_KEY);
  if (
    typeof current !== "string" ||
    !RETIRED_ANISETTE_SERVERS.has(hostOf(current))
  ) {
    return;
  }

  await store.set(ANISETTE_SERVER_KEY, DEFAULT_ANISETTE_SERVER);
  await store.save();
  migratedFrom = current;
}

/** The retired server this launch moved the user off, handed out once. */
export function takeRetiredAnisetteMigration(): string | null {
  const server = migratedFrom;
  migratedFrom = null;
  return server;
}

/**
 * Clears the stored anisette state if it was provisioned for another server or
 * under another identity. Resolves to whether there was one, i.e. whether the
 * next sign-in will ask for a 2FA code again.
 *
 * Can take a few seconds: the backend first fetches the published identity.
 */
export async function prepareAnisette(server: string): Promise<boolean> {
  try {
    return await invoke<boolean>("prepare_anisette", { anisetteServer: server });
  } catch (e) {
    // Sign-ins run the same check, so this only costs the early warning.
    console.error("Failed to prepare anisette state:", e);
    return false;
  }
}
