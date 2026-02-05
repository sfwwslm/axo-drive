import type { MessageKey } from "../i18n";
import LoginLogo from "./LoginLogo";

type AuthCheckingScreenProps = {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
};

// 登录态检查中的占位界面。
const AuthCheckingScreen = ({ t }: AuthCheckingScreenProps) => (
  <div className="app-shell login-shell">
    <div className="login-hero">
      <div className="login-brand">
        <span className="brand-title">AxoDrive</span>
        <p className="login-tagline">{t("tagline")}</p>
      </div>
      <section className="panel login-panel">
        <div className="login-logo-wrap">
          <LoginLogo className="login-logo" />
        </div>
        <p className="login-loading">{t("checkingAuth")}</p>
      </section>
    </div>
  </div>
);

export default AuthCheckingScreen;
