import { useEffect, useRef } from "react";

type LoginLogoProps = {
  className?: string;
  sleep?: boolean;
};

// 登录页 Logo（眼球跟随鼠标）。
const LoginLogo = ({ className, sleep = false }: LoginLogoProps) => {
  const svgRef = useRef<SVGSVGElement | null>(null);

  useEffect(() => {
    const handleMove = (event: MouseEvent) => {
      const svg = svgRef.current;
      if (!svg) return;
      if (svg.getAttribute("data-sleep") === "true") return;
      const rect = svg.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      const centerX = rect.left + rect.width / 2;
      const centerY = rect.top + rect.height / 2;
      const dx = event.clientX - centerX;
      const dy = event.clientY - centerY;
      const max = Math.min(rect.width, rect.height) / 2;
      const nx = Math.max(-1, Math.min(1, dx / max));
      const ny = Math.max(-1, Math.min(1, dy / max));
      const offset = 3.5;
      svg.style.setProperty("--eye-x", `${nx * offset}px`);
      svg.style.setProperty("--eye-y", `${ny * offset}px`);
    };

    window.addEventListener("mousemove", handleMove);
    return () => window.removeEventListener("mousemove", handleMove);
  }, []);

  return (
    <svg
      ref={svgRef}
      className={className}
      data-sleep={sleep ? "true" : "false"}
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 256 256"
      role="img"
      aria-label="AxoDrive"
    >
      <path
        d="M64 92C64 71.908 80.908 55 101 55H155C175.092 55 192 71.908 192 92V160C192 180.092 175.092 197 155 197H101C80.908 197 64 180.092 64 160V92Z"
        fill="#D4E5F6"
      />
      <path
        d="M128 74C160.078 74 186 99.922 186 132C186 164.078 160.078 190 128 190C95.922 190 70 164.078 70 132C70 99.922 95.922 74 128 74Z"
        fill="#F7FBFF"
      />
      <path
        d="M84 108L56 88"
        stroke="#3B82F6"
        strokeWidth="10"
        strokeLinecap="round"
      />
      <path
        d="M172 108L200 88"
        stroke="#3B82F6"
        strokeWidth="10"
        strokeLinecap="round"
      />
      <path
        d="M96 150C96 150 112 162 128 162C144 162 160 150 160 150"
        fill="none"
        stroke="#1D4ED8"
        strokeWidth="10"
        strokeLinecap="round"
      />
      <circle className="eye" cx="104" cy="124" r="8" fill="#1E3A8A" />
      <circle className="eye" cx="152" cy="124" r="8" fill="#1E3A8A" />
    </svg>
  );
};

export default LoginLogo;
