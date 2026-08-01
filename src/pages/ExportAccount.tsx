import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useStore } from "../StoreContext";

interface ExportAccountProps {
  email: string;
}

/**
 * Exports the signed-in account so 西瓜商店 can import it and skip its own
 * sign-in.
 *
 * The password is asked for again rather than kept around after login: this
 * screen is reached long after sign-in, and holding the password in renderer
 * state that whole time buys nothing.
 */
export const ExportAccount = ({ email }: ExportAccountProps) => {
  const { t } = useTranslation();
  const [anisetteServer] = useStore<string>("anisetteServer", "ani.sidestore.io");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  const doExport = async () => {
    if (!password) {
      toast.error(t("export_account.need_password"));
      return;
    }
    setBusy(true);
    try {
      const path = await invoke<string>("export_account_file", {
        email,
        password,
        anisetteServer,
      });
      setPassword("");
      toast.success(t("export_account.saved", { path }));
    } catch (e) {
      console.error("Failed to export account", e);
      toast.error(String(e));
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

      <label className="export-account-label" htmlFor="export-account-password">
        {t("export_account.password")}
      </label>
      <input
        id="export-account-password"
        type="password"
        value={password}
        autoComplete="off"
        onChange={(e) => setPassword(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !busy) doExport();
        }}
      />

      <button
        className="export-account-submit"
        onClick={doExport}
        disabled={busy || !password}
      >
        {busy ? t("export_account.working") : t("export_account.submit")}
      </button>

      <ol className="export-account-steps">
        <li>{t("export_account.step1")}</li>
        <li>{t("export_account.step2")}</li>
        <li>{t("export_account.step3")}</li>
      </ol>
    </div>
  );
};
