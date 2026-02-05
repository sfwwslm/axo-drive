import type { FormEvent } from "react";
import type { MessageKey } from "../i18n";
import LoginLogo from "./LoginLogo";

type LoginScreenProps = {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
  username: string;
  password: string;
  loggingIn: boolean;
  loginError: string | null;
  loginFocus: boolean;
  onUsernameChange: (value: string) => void;
  onPasswordChange: (value: string) => void;
  onLoginFocus: (focused: boolean) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
};

// 登录页表单与品牌区块。
const LoginScreen = ({
  t,
  username,
  password,
  loggingIn,
  loginError,
  loginFocus,
  onUsernameChange,
  onPasswordChange,
  onLoginFocus,
  onSubmit,
}: LoginScreenProps) => (
  <div className="app-shell login-shell">
    <div className="login-hero">
      <div className="login-brand">
        <span className="brand-title">AxoDrive</span>
        <p className="login-tagline">{t("tagline")}</p>
      </div>
      <section className="panel login-panel">
        <div className="login-logo-wrap">
          <LoginLogo className="login-logo" sleep={loginFocus} />
        </div>
        <form className="login-form" onSubmit={onSubmit}>
          <label>
            {t("username")}
            <input
              value={username}
              onChange={(event) => onUsernameChange(event.target.value)}
              onFocus={() => onLoginFocus(true)}
              onBlur={() => onLoginFocus(false)}
              autoComplete="username"
            />
          </label>
          <label>
            {t("password")}
            <input
              type="password"
              value={password}
              onChange={(event) => onPasswordChange(event.target.value)}
              onFocus={() => onLoginFocus(true)}
              onBlur={() => onLoginFocus(false)}
              autoComplete="current-password"
            />
          </label>
          {loginError && <p className="status error">{loginError}</p>}
          <div className="login-actions">
            <button type="submit" disabled={loggingIn}>
              {loggingIn ? t("loggingIn") : t("login")}
            </button>
          </div>
        </form>
      </section>
    </div>
  </div>
);

export default LoginScreen;
