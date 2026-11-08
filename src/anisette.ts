import { invoke } from "@tauri-apps/api/core";
import type { Store } from "@tauri-apps/plugin-store";

export const ANISETTE_SERVER_KEY = "anisetteServer";
export const DEFAULT_ANISETTE_SERVER = "ani.sidestore.io";

/**
 * Presets offered in Settings.
 *
 * isideload sends Apple whatever identity the server reports at
 * `/v3/client_info` (as `X-Mme-Client-Info`), and an exported account pins
 * 西瓜IPA助手 on the phone to the same server. So a server only belongs here if
 * it reports the akd identity (`com.apple.akd`): since around September 2026
 * Apple turns the Xcode one (`com.apple.dt.Xcode`) away with 503.
 * Last checked 2026-11-08.
 */
export const ANISETTE_SERVERS: [host: string, label: string][] = [
  [DEFAULT_ANISETTE_SERVER, "SideStore (.io)"],
  ["ani.stikstore.app", "StikStore"],
  ["ani.sidestore.app", "SideStore (.app)"],
];

/**
 * Presets dropped in 1.0.5. Removing them from the list alone would strand
 * whoever already picked one: the setting persists, and the dropdown shows an
 * unknown value as a custom server. See `migrateRetiredAnisetteServer`.
 */
const RETIRED_ANISETTE_SERVERS = new Set([
  "ani.sidestore.zip", // Xcode identity
  "ani.846969.xyz", // Xcode identity
  "ani.xu30.top", // Xcode identity
  "anisette.wedotstud.io", // Xcode identity
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
 * The anisette state is reset as well. isideload keeps a single adi.pb for
 * every server, and the one stored now was most likely provisioned at the
 * retired server. Starting fresh costs one 2FA code, but these users weren't
 * getting signed in anyway.
 */
export async function migrateRetiredAnisetteServer(store: Store): Promise<void> {
  const current = await store.get<string>(ANISETTE_SERVER_KEY);
  if (
    typeof current !== "string" ||
    !RETIRED_ANISETTE_SERVERS.has(hostOf(current))
  ) {
    return;
  }

  try {
    await invoke("reset_anisette_state");
  } catch (e) {
    // Switch anyway: staying on a server Apple rejects is the worse outcome,
    // and a stale adi.pb can still be cleared from Settings.
    console.error("Failed to reset anisette state for retired server:", e);
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
