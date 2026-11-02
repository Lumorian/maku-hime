import logoUrl from "../assets/logo.png";
import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function base({ size = 16, ...props }: IconProps) {
  return {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    ...props,
  };
}

export const IconSend = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M4 12l16-8-6 16-3-6-7-2z" />
  </svg>
);

export const IconSettings = (props: IconProps) => (
  <svg {...base(props)}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
  </svg>
);

export const IconHistory = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M3 12a9 9 0 1 0 3-6.7" />
    <path d="M3 4v4h4" />
    <path d="M12 8v4l3 2" />
  </svg>
);

export const IconRefresh = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M21 12a9 9 0 1 1-2.6-6.4" />
    <path d="M21 3v5h-5" />
  </svg>
);

export const IconPlus = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const IconTrash = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13" />
  </svg>
);

export const IconLock = (props: IconProps) => (
  <svg {...base(props)}>
    <rect x="4" y="10" width="16" height="10" rx="2" />
    <path d="M8 10V7a4 4 0 0 1 8 0v3" />
  </svg>
);

export const IconUnlock = (props: IconProps) => (
  <svg {...base(props)}>
    <rect x="4" y="10" width="16" height="10" rx="2" />
    <path d="M8 10V7a4 4 0 0 1 7-2.6" />
  </svg>
);

export const IconClose = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

export const IconWindow = (props: IconProps) => (
  <svg {...base(props)}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 9h18" />
  </svg>
);

export const IconCopy = (props: IconProps) => (
  <svg {...base(props)}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M5 15V5a1 1 0 0 1 1-1h10" />
  </svg>
);

/** 开播：一个带信号的发射塔。 */
export const IconBroadcast = (props: IconProps) => (
  <svg {...base(props)}>
    <circle cx="12" cy="12" r="2" />
    <path d="M7.8 16.2a6 6 0 0 1 0-8.4M16.2 7.8a6 6 0 0 1 0 8.4" />
    <path d="M5 19a10 10 0 0 1 0-14M19 5a10 10 0 0 1 0 14" />
  </svg>
);

export const IconEye = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12z" />
    <circle cx="12" cy="12" r="2.6" />
  </svg>
);

export const IconEyeOff = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M3 3l18 18" />
    <path d="M10.6 6.2A10.6 10.6 0 0 1 12 6c6.5 0 10 6 10 6a17.6 17.6 0 0 1-3.2 3.7M6.3 6.3A17.7 17.7 0 0 0 2 12s3.5 6 10 6a10.9 10.9 0 0 0 4.1-.8" />
  </svg>
);

export const IconQr = (props: IconProps) => (
  <svg {...base(props)}>
    <rect x="3" y="3" width="7" height="7" rx="1" />
    <rect x="14" y="3" width="7" height="7" rx="1" />
    <rect x="3" y="14" width="7" height="7" rx="1" />
    <path d="M14 14h3v3h-3zM20 14v3M14 20h3M20 20v.01" />
  </svg>
);

/** 直播间设置：滑杆。 */
export const IconSliders = (props: IconProps) => (
  <svg {...base(props)}>
    <path d="M4 7h9M17 7h3M4 12h3M11 12h9M4 17h9M17 17h3" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="12" r="2" />
    <circle cx="15" cy="17" r="2" />
  </svg>
);

/** 表情：笑脸。 */
export const IconSmile = (props: IconProps) => (
  <svg {...base(props)}>
    <circle cx="12" cy="12" r="9" />
    <path d="M8.5 14.2a4.6 4.6 0 0 0 7 0" />
    <path d="M9 9.6v.01M15 9.6v.01" />
  </svg>
);

/** 封面：一张图片。 */
export const IconImage = (props: IconProps) => (
  <svg {...base(props)}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <circle cx="8.5" cy="9.5" r="1.4" />
    <path d="M21 15.5l-4.5-4.5L10 17.5" />
  </svg>
);

export const IconSearch = (props: IconProps) => (
  <svg {...base(props)}>
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-3.5-3.5" />
  </svg>
);

/**
 * 品牌标记：舞台幕帘的垂幔读作小写 m（= maku-hime = 幕），
 * 右侧三道横线是飞过的弹幕，右上角的四角星是「姬」。
 *
 * 图形来自 `docs/logo/maku-hime-logo.png`，经 `scripts/gen-icons.py`
 * 裁成透明圆角的 256px 位图（`src/assets/logo.png`）。
 * 渐变底本身不可着色，所以这里用 <img> 而不是内联 SVG：
 * 缩放由位图分辨率兜底（256px 够撑到 4× 屏的 64px 显示）。
 */
export const Logo = ({ size = 28 }: { size?: number }) => (
  <img
    className="brand-logo"
    src={logoUrl}
    width={size}
    height={size}
    alt=""
    aria-hidden="true"
    decoding="async"
    draggable={false}
  />
);
