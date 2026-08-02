import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useStore } from "../StoreContext";
import { useError } from "../ErrorContext";
import { AppError } from "../errors";

interface ExportAccountProps {
  email: string;
}

/**
 * Exports the signed-in account so 西瓜IPA助手 can import it and skip its own
 * sign-in.
 *
 * The password is only asked for when it isn't already in the keychain —
 * prompting every time would defeat a feature whose whole point is one less
 * login.
 */
export const ExportAccount = ({ email }: ExportAccountProps) => {
  const { t } = useTranslation();
  const { err } = useError();
  const [anisetteServer] = useStore<string>("anisetteServer", "ani.sidestore.io");
  const [password, setPassword] = useState("");
  const [needsPassword, setNeedsPassword] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!email) return;
    invoke<boolean>("has_stored_password", { email })
      .then((stored) => setNeedsPassword(!stored))
      .catch(() => setNeedsPassword(true));
  }, [email]);

  const run = async (command: "send_account_to_device" | "export_account_file") => {
    if (needsPassword && !password) {
      toast.error(t("export_account.need_password"));
      return;
    }
    setBusy(true);
    try {
      const result = await invoke<string>(command, {
        email,
        password: password || null,
        anisetteServer,
      });
      setPassword("");
      toast.success(
        command === "send_account_to_device"
          ? t("export_account.sent", { app: result })
          : t("export_account.saved", { path: result }),
      );
    } catch (e) {
      console.error(`${command} failed`, e);
      // AppError serializes to {type, message}; String() on it yields
      // "[object Object]". Route it through useError like everywhere else so
      // the toast gets a readable line and the details panel gets suggestions.
      toast.error(err(t("export_account.failed"), e as AppError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="export-account">
      <h2>{t("export_account.title")}</h2>
      <p className="export-account-desc">{t("export_account.desc")}</p>

      <div className="export-account-warning">
        <strong>{t("export_account.warning_title")}</strong>
        <p>{t("export_account.warning_body")}</p>
      </div>

      <label className="export-account-label" htmlFor="export-account-email">
        Apple ID
      </label>
      <input id="export-account-email" type="text" value={email} readOnly />

      {needsPassword && (
        <>
          <label
            className="export-account-label"
            htmlFor="export-account-password"
          >
            {t("export_account.password")}
          </label>
          <input
            id="export-account-password"
            type="password"
            value={password}
            autoComplete="off"
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !busy) run("send_account_to_device");
            }}
          />
          <p className="export-account-hint">
            {t("export_account.why_password")}
          </p>
        </>
      )}

      <button
        className="export-account-submit"
        onClick={() => run("send_account_to_device")}
        disabled={busy || needsPassword === null || (needsPassword && !password)}
      >
        {busy ? t("export_account.working") : t("export_account.send")}
      </button>
      <p className="export-account-hint">{t("export_account.send_hint")}</p>

      <button
        className="export-account-secondary"
        onClick={() => run("export_account_file")}
        disabled={busy || needsPassword === null || (needsPassword && !password)}
      >
        {t("export_account.submit")}
      </button>
      <ol className="export-account-steps">
        <li>{t("export_account.step1")}</li>
        <li>{t("export_account.step2")}</li>
        <li>{t("export_account.step3")}</li>
      </ol>
    </div>
  );
};
